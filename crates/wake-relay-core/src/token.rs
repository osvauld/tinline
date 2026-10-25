//! Wake tokens: what a device hands a contact so the contact can wake it through the relay.
//!
//! A token is `base64url(kid[1] || nonce[12] || AES-256-GCM(relay key, postcard(WakeClaim)))`
//! under [`TOKEN_AAD`]. Only the relay can open it; the contact holding it learns nothing (not
//! the push token, not the tag). One token per (receiving device, contact): `tag` is random per
//! pair, so a block or a call-permission change revokes exactly one.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};

use crate::{DAY_MS, WakeError, b64};

pub const TOKEN_AAD: &[u8] = b"tinline/wake/token/v1";
/// Longest token the relay reads; real ones are a few hundred characters.
pub const MAX_TOKEN_CHARS: usize = 2048;
/// Longest lifetime the relay seals (the receiver renews at 30 days, PROVISIONAL).
pub const MAX_TOKEN_LIFE_MS: u64 = 31 * DAY_MS;
const NONCE_LEN: usize = 12;

/// What a wake is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Msg,
    Call,
}

impl Kind {
    fn bit(self) -> u8 {
        match self {
            Kind::Msg => 1,
            Kind::Call => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Msg => "msg",
            Kind::Call => "call",
        }
    }
}

/// The kinds a token may wake for. A contact without call permission gets [`Kinds::MSG`] only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kinds(u8);

impl Kinds {
    pub const MSG: Kinds = Kinds(1);
    pub const CALL: Kinds = Kinds(2);
    pub const ALL: Kinds = Kinds(3);

    pub fn allows(self, kind: Kind) -> bool {
        self.0 & kind.bit() != 0
    }

    pub fn is_valid(self) -> bool {
        self.0 != 0 && self.0 & !Self::ALL.0 == 0
    }

    /// The wire form (`proto::Msg::WakeTokens::kinds`): 1 message, 2 call, 3 both.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// From the wire form; `None` unless valid.
    pub fn from_bits(bits: u8) -> Option<Kinds> {
        Some(Kinds(bits)).filter(|k| k.is_valid())
    }
}

/// Where the push goes. Each push token is what the platform handed the device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Tinline (Play): FCM registration token.
    Fcm { token: String },
    /// iOS: the regular APNs token (message wakes) and the PushKit VoIP token (call wakes).
    /// `sandbox` for development builds (Xcode), which APNs serves from its sandbox host.
    Apns {
        alert: String,
        voip: Option<String>,
        sandbox: bool,
    },
    /// Tinline Liberum: the UnifiedPush endpoint URL of the user's distributor.
    UnifiedPush { endpoint: String },
}

impl Target {
    /// Syntactic checks; the platform has the last word on whether a token is live.
    pub fn validate(&self) -> Result<(), WakeError> {
        match self {
            Target::Fcm { token } => {
                check_token_chars(token, 16, 4096).map_err(|_| WakeError::BadTarget("fcm token"))
            }
            Target::Apns { alert, voip, .. } => {
                check_apns(alert)?;
                voip.as_deref().map_or(Ok(()), check_apns)
            }
            Target::UnifiedPush { endpoint } => check_endpoint(endpoint),
        }
    }

    /// Whether a push of `kind` can be delivered to this target at all.
    pub fn supports(&self, kind: Kind) -> bool {
        match (self, kind) {
            (Target::Apns { voip: None, .. }, Kind::Call) => false,
            _ => true,
        }
    }
}

fn check_token_chars(token: &str, min: usize, max: usize) -> Result<(), ()> {
    let ok = (min..=max).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'));
    if ok { Ok(()) } else { Err(()) }
}

fn check_apns(token: &str) -> Result<(), WakeError> {
    let ok = (32..=200).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(WakeError::BadTarget("apns token"))
    }
}

/// An https URL on the default port with a host name, so the relay only ever POSTs to a
/// distributor's push server, never to an address literal or an odd port.
fn check_endpoint(endpoint: &str) -> Result<(), WakeError> {
    let bad = WakeError::BadTarget("unifiedpush endpoint");
    if endpoint.len() > 1024 || endpoint.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(bad);
    }
    let rest = endpoint.strip_prefix("https://").ok_or(bad.clone())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', ':', '[', ']']) {
        return Err(bad);
    }
    if !authority.contains('.') || authority.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(bad);
    }
    Ok(())
}

/// What a wake token says; only the relay reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeClaim {
    pub target: Target,
    /// The receiving device's key for [`crate::hint`]; the relay seals each hint with it.
    pub hint_key: [u8; 32],
    /// Random per (receiving device, contact); names the contact to the receiver only.
    pub tag: [u8; 16],
    pub kinds: Kinds,
    pub exp_ms: u64,
}

impl WakeClaim {
    /// May this token wake for `kind` now?
    pub fn check(&self, kind: Kind, now_ms: u64) -> Result<(), WakeError> {
        if now_ms >= self.exp_ms {
            return Err(WakeError::Expired);
        }
        if !self.kinds.allows(kind) || !self.target.supports(kind) {
            return Err(WakeError::KindNotAllowed);
        }
        Ok(())
    }

    /// Key for the relay's revoke list and rate limits. Tags are random and secret to the relay
    /// and the receiver, so the hex tag is enough.
    pub fn tag_id(&self) -> String {
        self.tag.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// One of the relay's sealing keys. `kid` lets the relay rotate: it seals with the newest and
/// opens with any it still holds.
#[derive(Clone)]
pub struct RelayKey {
    pub kid: u8,
    pub key: [u8; 32],
}

impl std::fmt::Debug for RelayKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayKey")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

/// Seals `claim` under `key`. `nonce` must be fresh random bytes (the Worker draws them).
pub fn seal(
    key: &RelayKey,
    claim: &WakeClaim,
    nonce: [u8; NONCE_LEN],
) -> Result<String, WakeError> {
    let body = postcard::to_allocvec(claim).map_err(|_| WakeError::Malformed)?;
    let cipher = Aes256Gcm::new_from_slice(&key.key).map_err(|_| WakeError::Sealed)?;
    let sealed = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &body,
                aad: &aad(key.kid),
            },
        )
        .map_err(|_| WakeError::Sealed)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
    out.push(key.kid);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(b64::encode(&out))
}

/// Opens a token sealed with any of `keys`.
pub fn open(keys: &[RelayKey], token: &str) -> Result<WakeClaim, WakeError> {
    if token.len() > MAX_TOKEN_CHARS {
        return Err(WakeError::TooLong);
    }
    let raw = b64::decode(token).ok_or(WakeError::Malformed)?;
    if raw.len() < 1 + NONCE_LEN {
        return Err(WakeError::Malformed);
    }
    let (kid, rest) = (raw[0], &raw[1..]);
    let key = keys
        .iter()
        .find(|k| k.kid == kid)
        .ok_or(WakeError::UnknownKey)?;
    let (nonce, sealed) = rest.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(&key.key).map_err(|_| WakeError::Sealed)?;
    let nonce = Nonce::try_from(nonce).map_err(|_| WakeError::Malformed)?;
    let body = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: sealed,
                aad: &aad(kid),
            },
        )
        .map_err(|_| WakeError::Sealed)?;
    postcard::from_bytes(&body).map_err(|_| WakeError::Malformed)
}

fn aad(kid: u8) -> Vec<u8> {
    let mut aad = TOKEN_AAD.to_vec();
    aad.push(kid);
    aad
}
