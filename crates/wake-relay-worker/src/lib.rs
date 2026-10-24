//! The push wake relay on Cloudflare Workers (`docs/push.md` "Relay"). Every decision is in
//! `wake-relay-core`; this file does the HTTP, the KV revoke list, the rate limits, randomness
//! and WebCrypto signing.
//!
//! Bindings (`wrangler.toml`): KV `WAKE_KV`; rate limiters `MSG_LIMITER`, `CALL_LIMITER`,
//! `REGISTER_LIMITER`; vars `APNS_TOPIC`, `PROBE`; secrets `RELAY_KEYS`, `FCM_SERVICE_ACCOUNT`,
//! `APNS_KEY_P8`, `APNS_KEY_ID`, `APNS_TEAM_ID`. Nothing is logged about tokens or callers.

use std::cell::RefCell;

use js_sys::{Array, Object, Reflect, Uint8Array};
use wake_relay_core::api::{MAX_BODY_BYTES, RegisterRequest, RevokeRequest, WakeRequest};
use wake_relay_core::push::{self, Auth, HttpRequest, Outcome, PushConfig};
use wake_relay_core::{Kind, RelayKey, WakeError, jwt, relay, token};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use worker::{
    Context, Date, Env, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Response, Result,
    RouteContext, Router, event,
};

const KV: &str = "WAKE_KV";
const FCM_BEARER_KEY: &str = "auth:fcm";
const APNS_BEARER_KEY: &str = "auth:apns";
/// KV refuses shorter expirations.
const KV_MIN_TTL_S: u64 = 60;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get("/v1/health", |_, _| Response::ok("ok"))
        .post_async("/v1/register", |req, ctx| async move { answer(register(req, &ctx).await) })
        .post_async("/v1/wake", |req, ctx| async move { answer(wake(req, &ctx).await) })
        .post_async("/v1/revoke", |req, ctx| async move { answer(revoke(req, &ctx).await) })
        .get_async("/v1/probe/apns", |_, ctx| async move { answer(probe_apns(&ctx).await) })
        .run(req, env)
        .await
}

/// Why a request failed, before it becomes an HTTP status.
enum Fail {
    Wake(WakeError),
    Revoked,
    Limited,
    TooLarge,
    /// The platform's answer for the sender ([`Outcome`] other than `Sent`).
    Platform(Outcome),
    Internal(String),
}

impl From<WakeError> for Fail {
    fn from(e: WakeError) -> Self {
        Fail::Wake(e)
    }
}

impl From<worker::Error> for Fail {
    fn from(e: worker::Error) -> Self {
        Fail::Internal(e.to_string())
    }
}

fn js(e: JsValue) -> Fail {
    Fail::Internal(format!("{e:?}"))
}

fn answer(result: std::result::Result<Response, Fail>) -> Result<Response> {
    match result {
        Ok(r) => Ok(r),
        Err(Fail::Wake(e)) => Response::error(e.to_string(), e.status()),
        Err(Fail::Revoked) => Response::error("revoked", 410),
        Err(Fail::Limited) => Response::error("limited", 429),
        Err(Fail::TooLarge) => Response::error("too large", 413),
        Err(Fail::Platform(Outcome::Gone)) => Response::error("gone", 410),
        Err(Fail::Platform(Outcome::Retry)) => Response::error("try later", 503),
        Err(Fail::Platform(o)) => {
            worker::console_error!("platform refused a wake: {o:?}");
            Response::error("platform refused", 502)
        }
        Err(Fail::Internal(msg)) => {
            worker::console_error!("internal: {msg}");
            Response::error("internal", 500)
        }
    }
}

// ---- endpoints

async fn register(mut req: Request, ctx: &RouteContext<()>) -> std::result::Result<Response, Fail> {
    let ip = req.headers().get("cf-connecting-ip")?.unwrap_or_default();
    limit(ctx, "REGISTER_LIMITER", ip).await?;
    let body: RegisterRequest = read_json(&mut req).await?;
    let keys = relay_keys(ctx)?;
    let now = now_ms();
    let mut pool = random_bytes(12 * body.tags.len().min(relay::MAX_TAGS_PER_REGISTER))?.into_iter();
    let nonce = move || {
        let mut n = [0u8; 12];
        n.iter_mut().for_each(|b| *b = pool.next().unwrap_or(0));
        n
    };
    let resp = relay::register(&keys[0], &body, now, nonce)?;
    Ok(Response::from_json(&resp)?)
}

async fn wake(mut req: Request, ctx: &RouteContext<()>) -> std::result::Result<Response, Fail> {
    let body: WakeRequest = read_json(&mut req).await?;
    let keys = relay_keys(ctx)?;
    let now = now_ms();
    let claim = relay::open_for_wake(&keys, &body, now)?;
    if kv(ctx)?.get(&relay::revoke_key(&claim)).text().await.map_err(kv_err)?.is_some() {
        return Err(Fail::Revoked);
    }
    let limiter = match body.kind {
        Kind::Msg => "MSG_LIMITER",
        Kind::Call => "CALL_LIMITER",
    };
    limit(ctx, limiter, relay::rate_key(&claim, body.kind)).await?;

    let cfg = push_config(ctx)?;
    let request = relay::wake_request(&cfg, &claim, body.kind, now, random_array()?)?;
    let mut outcome = deliver(ctx, &claim.target, &request).await?;
    if outcome == Outcome::AuthFailed {
        // A cached bearer went stale early (key rotated, clock skew): mint a fresh one once.
        forget_bearer(ctx, request.auth).await?;
        outcome = deliver(ctx, &claim.target, &request).await?;
    }
    match outcome {
        Outcome::Sent => Ok(Response::empty()?.with_status(202)),
        other => Err(Fail::Platform(other)),
    }
}

async fn revoke(mut req: Request, ctx: &RouteContext<()>) -> std::result::Result<Response, Fail> {
    let body: RevokeRequest = read_json(&mut req).await?;
    let claim = match token::open(&relay_keys(ctx)?, &body.token) {
        Ok(c) => c,
        // A token no key opens any more can wake nobody; nothing to keep.
        Err(WakeError::UnknownKey) => return Ok(Response::empty()?.with_status(204)),
        Err(e) => return Err(e.into()),
    };
    let now_s = now_ms() / 1000;
    let exp_s = claim.exp_ms / 1000;
    if exp_s > now_s {
        let ttl = (exp_s - now_s + 1).max(KV_MIN_TTL_S);
        kv(ctx)?
            .put(&relay::revoke_key(&claim), "1")
            .map_err(kv_err)?
            .expiration_ttl(ttl)
            .execute()
            .await
            .map_err(kv_err)?;
    }
    Ok(Response::empty()?.with_status(204))
}

/// Whether Apple accepts a connection from this Worker (HTTP/2). Sends one push with a fake
/// provider token to a fake device on the sandbox host; an answer from APNs, such as 403
/// `InvalidProviderToken`, means the connection works. On only while var `PROBE` is "1".
async fn probe_apns(ctx: &RouteContext<()>) -> std::result::Result<Response, Fail> {
    if ctx.env.var("PROBE").map(|v| v.to_string()).unwrap_or_default() != "1" {
        return Ok(Response::error("not found", 404)?);
    }
    let headers = Headers::new();
    headers.set("authorization", "bearer e30.e30.AAAA")?;
    headers.set("apns-push-type", "alert")?;
    headers.set("apns-topic", "com.osvauld.tinline")?;
    let request = post(&format!("{}/3/device/{}", push::APNS_SANDBOX, "00".repeat(32)), headers, b"{}")?;
    let result = match Fetch::Request(request).send().await {
        Ok(mut r) => format!("apns answered {} {}", r.status_code(), r.text().await.unwrap_or_default()),
        Err(e) => format!("no answer: {e}"),
    };
    Ok(Response::ok(result)?)
}

// ---- delivery

async fn deliver(
    ctx: &RouteContext<()>,
    target: &wake_relay_core::Target,
    req: &HttpRequest,
) -> std::result::Result<Outcome, Fail> {
    let headers = Headers::new();
    for (k, v) in &req.headers {
        headers.set(k, v)?;
    }
    match req.auth {
        Auth::None => {}
        Auth::Fcm => headers.set("authorization", &format!("Bearer {}", fcm_bearer(ctx).await?))?,
        Auth::Apns => headers.set("authorization", &format!("bearer {}", apns_bearer(ctx).await?))?,
    }
    let mut resp = Fetch::Request(post(&req.url, headers, &req.body)?).send().await?;
    let status = resp.status_code();
    let text = if (200..300).contains(&status) { String::new() } else { resp.text().await.unwrap_or_default() };
    if !matches!(req.auth, Auth::None) && !(200..300).contains(&status) {
        // FCM/APNs error bodies name the reason (PERMISSION_DENIED, BadDeviceToken); no content.
        let head: String = text.chars().take(300).collect();
        worker::console_error!("provider answered {status}: {head}");
    }
    Ok(push::outcome(target, status, &text))
}

fn post(url: &str, headers: Headers, body: &[u8]) -> std::result::Result<Request, Fail> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        // Never follow a distributor's redirect somewhere else.
        .with_redirect(RequestRedirect::Manual)
        .with_body(Some(Uint8Array::from(body).into()));
    Ok(Request::new_with_init(url, &init)?)
}

// ---- provider bearers: cached in the isolate and in KV, so isolates share one APNs token
// (Apple refuses new ones more often than every 20 minutes).

thread_local! {
    static BEARERS: RefCell<[Option<(String, u64)>; 2]> = const { RefCell::new([None, None]) };
}

fn slot(auth: Auth) -> (usize, &'static str) {
    match auth {
        Auth::Apns => (1, APNS_BEARER_KEY),
        _ => (0, FCM_BEARER_KEY),
    }
}

async fn cached_bearer(ctx: &RouteContext<()>, auth: Auth) -> std::result::Result<Option<String>, Fail> {
    let (i, key) = slot(auth);
    let now_s = now_ms() / 1000;
    let local = BEARERS.with(|b| b.borrow()[i].clone()).filter(|(_, until)| *until > now_s);
    if let Some((token, _)) = local {
        return Ok(Some(token));
    }
    Ok(kv(ctx)?.get(key).text().await.map_err(kv_err)?)
}

async fn store_bearer(ctx: &RouteContext<()>, auth: Auth, token: &str, life_s: u64) -> std::result::Result<(), Fail> {
    let (i, key) = slot(auth);
    let until = now_ms() / 1000 + life_s;
    BEARERS.with(|b| b.borrow_mut()[i] = Some((token.to_string(), until)));
    if life_s >= KV_MIN_TTL_S {
        kv(ctx)?.put(key, token).map_err(kv_err)?.expiration_ttl(life_s).execute().await.map_err(kv_err)?;
    }
    Ok(())
}

async fn forget_bearer(ctx: &RouteContext<()>, auth: Auth) -> std::result::Result<(), Fail> {
    if auth == Auth::None {
        return Ok(());
    }
    let (i, key) = slot(auth);
    BEARERS.with(|b| b.borrow_mut()[i] = None);
    kv(ctx)?.delete(key).await.map_err(kv_err)?;
    Ok(())
}

async fn fcm_bearer(ctx: &RouteContext<()>) -> std::result::Result<String, Fail> {
    if let Some(t) = cached_bearer(ctx, Auth::Fcm).await? {
        return Ok(t);
    }
    let sa = service_account(ctx)?;
    let input = jwt::fcm_assertion_input(&sa, now_ms() / 1000);
    let sig = sign(&jwt::pkcs8_der(&sa.private_key)?, Alg::Rs256, input.as_bytes()).await?;
    let req = jwt::fcm_token_request(&sa, &jwt::finish(&input, &sig));
    let mut resp = Fetch::Request(post(&req.url, header_list(&req.headers)?, &req.body)?).send().await?;
    let text = resp.text().await?;
    if resp.status_code() != 200 {
        return Err(Fail::Internal(format!("fcm token exchange answered {}", resp.status_code())));
    }
    let (token, expires_in) = jwt::fcm_token_response(&text)?;
    store_bearer(ctx, Auth::Fcm, &token, expires_in.saturating_sub(jwt::FCM_REFRESH_MARGIN_S)).await?;
    Ok(token)
}

async fn apns_bearer(ctx: &RouteContext<()>) -> std::result::Result<String, Fail> {
    if let Some(t) = cached_bearer(ctx, Auth::Apns).await? {
        return Ok(t);
    }
    let input = jwt::apns_input(&secret(ctx, "APNS_TEAM_ID")?, &secret(ctx, "APNS_KEY_ID")?, now_ms() / 1000);
    let sig = sign(&jwt::pkcs8_der(&secret(ctx, "APNS_KEY_P8")?)?, Alg::Es256, input.as_bytes()).await?;
    let token = jwt::finish(&input, &sig);
    store_bearer(ctx, Auth::Apns, &token, jwt::APNS_JWT_REUSE_S).await?;
    Ok(token)
}

fn header_list(list: &[(String, String)]) -> std::result::Result<Headers, Fail> {
    let headers = Headers::new();
    for (k, v) in list {
        headers.set(k, v)?;
    }
    Ok(headers)
}

// ---- WebCrypto

#[derive(Clone, Copy)]
enum Alg {
    /// APNs: ECDSA P-256 / SHA-256; WebCrypto returns raw `r || s`, as JWT wants.
    Es256,
    /// FCM service account: RSASSA-PKCS1-v1_5 / SHA-256.
    Rs256,
}

async fn sign(pkcs8: &[u8], alg: Alg, data: &[u8]) -> std::result::Result<Vec<u8>, Fail> {
    let subtle = crypto()?.subtle();
    let (import, signing) = (Object::new(), Object::new());
    match alg {
        Alg::Es256 => {
            set(&import, "name", "ECDSA")?;
            set(&import, "namedCurve", "P-256")?;
            set(&signing, "name", "ECDSA")?;
            set(&signing, "hash", "SHA-256")?;
        }
        Alg::Rs256 => {
            set(&import, "name", "RSASSA-PKCS1-v1_5")?;
            set(&import, "hash", "SHA-256")?;
            set(&signing, "name", "RSASSA-PKCS1-v1_5")?;
        }
    }
    let usages = Array::of1(&JsValue::from_str("sign"));
    let der = Uint8Array::from(pkcs8);
    let key = JsFuture::from(subtle.import_key_with_object("pkcs8", &der, &import, false, &usages).map_err(js)?)
        .await
        .map_err(js)?;
    let sig = JsFuture::from(subtle.sign_with_object_and_u8_array(&signing, &key.unchecked_into(), data).map_err(js)?)
        .await
        .map_err(js)?;
    Ok(Uint8Array::new(&sig).to_vec())
}

fn set(obj: &Object, key: &str, value: &str) -> std::result::Result<(), Fail> {
    Reflect::set(obj, &JsValue::from_str(key), &JsValue::from_str(value)).map_err(js)?;
    Ok(())
}

fn crypto() -> std::result::Result<web_sys::Crypto, Fail> {
    js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>().crypto().map_err(js)
}

fn random_bytes(n: usize) -> std::result::Result<Vec<u8>, Fail> {
    let mut out = vec![0u8; n];
    // getRandomValues takes at most 65536 bytes per call.
    for chunk in out.chunks_mut(65536) {
        crypto()?.get_random_values_with_u8_array(chunk).map_err(js)?;
    }
    Ok(out)
}

fn random_array<const N: usize>() -> std::result::Result<[u8; N], Fail> {
    let v = random_bytes(N)?;
    let mut out = [0u8; N];
    out.copy_from_slice(&v);
    Ok(out)
}

// ---- configuration and helpers

fn now_ms() -> u64 {
    Date::now().as_millis()
}

fn secret(ctx: &RouteContext<()>, name: &str) -> std::result::Result<String, Fail> {
    Ok(ctx.env.secret(name)?.to_string())
}

fn service_account(ctx: &RouteContext<()>) -> std::result::Result<jwt::ServiceAccount, Fail> {
    Ok(jwt::ServiceAccount::from_json(&secret(ctx, "FCM_SERVICE_ACCOUNT")?)?)
}

fn push_config(ctx: &RouteContext<()>) -> std::result::Result<PushConfig, Fail> {
    // The FCM project is the service account's; without FCM configured (iOS-only test) it is empty.
    let fcm_project = service_account(ctx).map(|sa| sa.project_id).unwrap_or_default();
    Ok(PushConfig { fcm_project, apns_topic: ctx.env.var("APNS_TOPIC")?.to_string() })
}

/// `RELAY_KEYS`: `kid:base64url(32 bytes)` entries, comma-separated, newest (the sealing key) first.
fn relay_keys(ctx: &RouteContext<()>) -> std::result::Result<Vec<RelayKey>, Fail> {
    let text = secret(ctx, "RELAY_KEYS")?;
    let keys: Option<Vec<RelayKey>> = text
        .split(',')
        .map(|entry| {
            let (kid, key) = entry.trim().split_once(':')?;
            let key = base64url(key)?;
            Some(RelayKey { kid: kid.parse().ok()?, key: key.try_into().ok()? })
        })
        .collect();
    keys.filter(|k| !k.is_empty()).ok_or_else(|| Fail::Internal("RELAY_KEYS malformed".into()))
}

fn base64url(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(text).ok()
}

fn kv(ctx: &RouteContext<()>) -> std::result::Result<worker::kv::KvStore, Fail> {
    Ok(ctx.env.kv(KV)?)
}

fn kv_err(e: worker::kv::KvError) -> Fail {
    Fail::Internal(format!("kv: {e:?}"))
}

async fn limit(ctx: &RouteContext<()>, binding: &str, key: String) -> std::result::Result<(), Fail> {
    if ctx.env.rate_limiter(binding)?.limit(key).await?.success {
        Ok(())
    } else {
        Err(Fail::Limited)
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(req: &mut Request) -> std::result::Result<T, Fail> {
    let body = req.bytes().await?;
    if body.len() > MAX_BODY_BYTES {
        return Err(Fail::TooLarge);
    }
    serde_json::from_slice(&body).map_err(|_| Fail::Wake(WakeError::Malformed))
}
