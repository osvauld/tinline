//! The platform requests for one wake, and how to read the platform's answer.
//!
//! Every push carries only the sealed hint (`h`). Calls expire fast (a late ring is useless);
//! message wakes collapse, so a burst shows as one wake-up.

use serde_json::json;

use crate::token::{Kind, Target};

/// Where APNs development builds (Xcode) and production builds are served.
pub const APNS_PROD: &str = "https://api.push.apple.com";
pub const APNS_SANDBOX: &str = "https://api.sandbox.push.apple.com";
pub const FCM_SEND: &str = "https://fcm.googleapis.com/v1/projects";
/// The fixed text of an iOS message wake (iOS must show something; it never names anyone).
pub const APNS_MSG_TEXT: &str = "New message";
const COLLAPSE_MSG: &str = "wake-msg";
/// How long a platform may hold an undelivered wake (PROVISIONAL, docs/push.md).
const CALL_TTL_S: u64 = 30;
const MSG_TTL_S: u64 = 3600;

/// Which bearer the Worker adds as `authorization` (it holds and caches both).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    None,
    /// OAuth access token from the FCM service account ([`crate::jwt::fcm_assertion_input`]).
    Fcm,
    /// The APNs provider JWT ([`crate::jwt::apns_input`]).
    Apns,
}

/// An HTTP POST to make; no I/O here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub auth: Auth,
}

/// The relay's fixed settings.
#[derive(Clone, Debug)]
pub struct PushConfig {
    pub fcm_project: String,
    /// The iOS bundle ID; VoIP pushes use `<topic>.voip`.
    pub apns_topic: String,
}

/// The request that delivers `hint` to `target` for `kind`. The caller has already checked the
/// token ([`crate::WakeClaim::check`]), so an APNs call wake has a VoIP token.
pub fn request(
    cfg: &PushConfig,
    target: &Target,
    kind: Kind,
    hint: &str,
    now_ms: u64,
) -> Option<HttpRequest> {
    let ttl = match kind {
        Kind::Call => CALL_TTL_S,
        Kind::Msg => MSG_TTL_S,
    };
    match target {
        Target::Fcm { token } => {
            let mut android = json!({ "priority": "HIGH", "ttl": format!("{ttl}s") });
            if kind == Kind::Msg {
                android["collapse_key"] = json!(COLLAPSE_MSG);
            }
            let body =
                json!({ "message": { "token": token, "data": { "h": hint }, "android": android } });
            Some(HttpRequest {
                url: format!("{FCM_SEND}/{}/messages:send", cfg.fcm_project),
                headers: vec![("content-type".into(), "application/json".into())],
                body: body.to_string().into_bytes(),
                auth: Auth::Fcm,
            })
        }
        Target::Apns {
            alert,
            voip,
            sandbox,
        } => {
            let host = if *sandbox { APNS_SANDBOX } else { APNS_PROD };
            let (device, body, headers) = match kind {
                Kind::Call => (
                    voip.as_deref()?,
                    json!({ "h": hint }),
                    vec![
                        ("apns-push-type", "voip".to_string()),
                        ("apns-topic", format!("{}.voip", cfg.apns_topic)),
                        ("apns-priority", "10".into()),
                        // 0: deliver now or never; a stored call wake would ring too late.
                        ("apns-expiration", "0".into()),
                    ],
                ),
                Kind::Msg => (
                    alert.as_str(),
                    json!({
                        "aps": { "alert": { "body": APNS_MSG_TEXT }, "sound": "default", "content-available": 1 },
                        "h": hint,
                    }),
                    vec![
                        ("apns-push-type", "alert".to_string()),
                        ("apns-topic", cfg.apns_topic.clone()),
                        ("apns-priority", "10".into()),
                        ("apns-expiration", (now_ms / 1000 + ttl).to_string()),
                        ("apns-collapse-id", COLLAPSE_MSG.into()),
                    ],
                ),
            };
            let mut headers: Vec<(String, String)> = headers
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
            headers.push(("content-type".into(), "application/json".into()));
            Some(HttpRequest {
                url: format!("{host}/3/device/{device}"),
                headers,
                body: body.to_string().into_bytes(),
                auth: Auth::Apns,
            })
        }
        Target::UnifiedPush { endpoint } => Some(HttpRequest {
            url: endpoint.clone(),
            headers: vec![
                ("content-type".into(), "application/octet-stream".into()),
                ("ttl".into(), ttl.to_string()),
                ("urgency".into(), "high".into()),
            ],
            body: hint.as_bytes().to_vec(),
            auth: Auth::None,
        }),
    }
}

/// What the platform's answer means for the sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// The push token is dead (app uninstalled, token replaced): the sender drops the wake token.
    Gone,
    /// Platform busy or down: the sender falls back to its normal retries.
    Retry,
    /// Our credentials were refused: the cached bearer is dropped and the wake retried once.
    AuthFailed,
    /// Anything else (a bug on our side, or a token the platform rejects outright).
    Rejected,
}

/// Reads a platform answer. `body` matters only for the few statuses whose meaning depends on it.
pub fn outcome(target: &Target, status: u16, body: &str) -> Outcome {
    match status {
        200..=299 => Outcome::Sent,
        429 | 500..=599 => Outcome::Retry,
        _ => match target {
            Target::Fcm { .. } => match status {
                404 => Outcome::Gone,
                401 | 403 => Outcome::AuthFailed,
                400 if body.contains("INVALID_ARGUMENT") && body.contains("registration token") => {
                    Outcome::Gone
                }
                _ => Outcome::Rejected,
            },
            Target::Apns { .. } => match status {
                410 => Outcome::Gone,
                400 if body.contains("BadDeviceToken")
                    || body.contains("DeviceTokenNotForTopic") =>
                {
                    Outcome::Gone
                }
                403 if body.contains("ExpiredProviderToken")
                    || body.contains("InvalidProviderToken") =>
                {
                    Outcome::AuthFailed
                }
                _ => Outcome::Rejected,
            },
            Target::UnifiedPush { .. } => match status {
                404 | 410 => Outcome::Gone,
                _ => Outcome::Rejected,
            },
        },
    }
}
