//! The two provider auth tokens, as JWTs whose signature is made elsewhere.
//!
//! The Worker signs with the runtime's WebCrypto (constant-time, no RSA crate in wasm): ECDSA
//! P-256 for APNs (WebCrypto returns raw `r || s`, which is what ES256 wants) and RSASSA-PKCS1-v1_5
//! SHA-256 for the FCM service account. Here: the signing input, the finished token, the PEM
//! unwrapping and the OAuth exchange around them.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::json;

use crate::push::{Auth, HttpRequest};
use crate::{WakeError, b64};

/// Apple refuses a new provider token more often than every 20 minutes and an old one after
/// 60; reuse each for this long.
pub const APNS_JWT_REUSE_S: u64 = 45 * 60;
/// Refresh the FCM access token this long before Google says it expires.
pub const FCM_REFRESH_MARGIN_S: u64 = 5 * 60;
pub const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const FCM_ASSERTION_LIFE_S: u64 = 3600;

/// `header.claims` for the APNs provider token.
pub fn apns_input(team_id: &str, key_id: &str, iat_s: u64) -> String {
    let header = json!({ "alg": "ES256", "kid": key_id });
    let claims = json!({ "iss": team_id, "iat": iat_s });
    signing_input(&header, &claims)
}

/// The parts of the Google service-account JSON the relay needs (the `wake-relay` account,
/// role "Firebase Cloud Messaging API Admin" only).
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key_id: String,
    /// PKCS#8 PEM.
    pub private_key: String,
    pub token_uri: String,
    pub project_id: String,
}

impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("client_email", &self.client_email)
            .finish_non_exhaustive()
    }
}

impl ServiceAccount {
    pub fn from_json(text: &str) -> Result<Self, WakeError> {
        serde_json::from_str(text).map_err(|_| WakeError::BadRequest("service account json"))
    }
}

/// `header.claims` for the OAuth assertion that buys an FCM access token.
pub fn fcm_assertion_input(sa: &ServiceAccount, iat_s: u64) -> String {
    let header = json!({ "alg": "RS256", "typ": "JWT", "kid": sa.private_key_id });
    let claims = json!({
        "iss": sa.client_email,
        "scope": FCM_SCOPE,
        "aud": sa.token_uri,
        "iat": iat_s,
        "exp": iat_s + FCM_ASSERTION_LIFE_S,
    });
    signing_input(&header, &claims)
}

/// `input.signature`: the finished JWT.
pub fn finish(input: &str, signature: &[u8]) -> String {
    format!("{input}.{}", b64::encode(signature))
}

/// The token exchange: POST the signed assertion to the service account's `token_uri`.
pub fn fcm_token_request(sa: &ServiceAccount, assertion: &str) -> HttpRequest {
    let body = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
    );
    HttpRequest {
        url: sa.token_uri.clone(),
        headers: vec![(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        body: body.into_bytes(),
        auth: Auth::None,
    }
}

/// `(access_token, expires_in_s)` from Google's answer.
pub fn fcm_token_response(body: &str) -> Result<(String, u64), WakeError> {
    #[derive(Deserialize)]
    struct Answer {
        access_token: String,
        expires_in: u64,
    }
    let a: Answer = serde_json::from_str(body).map_err(|_| WakeError::Malformed)?;
    Ok((a.access_token, a.expires_in))
}

/// The DER inside a `-----BEGIN PRIVATE KEY-----` block (an APNs `.p8` file, or a service
/// account's `private_key`), for WebCrypto's `importKey("pkcs8", …)`.
pub fn pkcs8_der(pem: &str) -> Result<Vec<u8>, WakeError> {
    const BEGIN: &str = "-----BEGIN PRIVATE KEY-----";
    const END: &str = "-----END PRIVATE KEY-----";
    let start = pem.find(BEGIN).ok_or(WakeError::Malformed)? + BEGIN.len();
    let end = pem[start..].find(END).ok_or(WakeError::Malformed)? + start;
    let body: String = pem[start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    STANDARD.decode(body).map_err(|_| WakeError::Malformed)
}

fn signing_input(header: &serde_json::Value, claims: &serde_json::Value) -> String {
    format!(
        "{}.{}",
        b64::encode(header.to_string().as_bytes()),
        b64::encode(claims.to_string().as_bytes())
    )
}
