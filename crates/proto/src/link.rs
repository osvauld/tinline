//! Device linking (docs/design/device-linking.md §1, §2): the QR, the proofs both sides
//! exchange, the sealed grant, and the messages of the separate `tinline/link/1` protocol.
//! Pure like the rest of proto; the mnemonic travels inside `LinkGrantBody`.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::*;

pub const LINK_ALPN: &[u8] = b"tinline/link/1";
const LINK_PREFIX: &str = "OSVL1:";
const LINK_VERSION: u8 = 1;
pub const LINK_TTL_SECS: u64 = 300;
const T_PREFIX: &[u8] = b"tinline/link/v1";
const NONCE_LEN: usize = 12;

// ---- QR ----------------------------------------------------------------------------

/// What the displaying device shows. A bearer secret for a few minutes; not signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkQr {
    pub device: [u8; 32],
    pub secret: [u8; 16],
    /// Unix seconds (minute resolution on the wire, rounded up when issuing).
    pub exp: u64,
    pub relay: Option<String>,
}

/// Fresh random secret, expiring `ttl_secs` after `now` (use `LINK_TTL_SECS`).
pub fn new_link_qr(device: [u8; 32], relay: Option<String>, now: u64, ttl_secs: u64) -> LinkQr {
    let mut secret = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    let exp_min = now
        .saturating_add(ttl_secs)
        .saturating_sub(V2_EPOCH)
        .div_ceil(60)
        .min(0xFF_FFFF);
    LinkQr {
        device,
        secret,
        exp: V2_EPOCH + exp_min * 60,
        relay: relay.filter(|r| r.len() <= 255),
    }
}

impl LinkQr {
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.exp
    }

    /// `OSVL1:` + base32 (uppercase, no padding) of
    /// `v | device[32] | secret[16] | exp[3] | relay_code<<5 [| len | url]`.
    pub fn to_text(&self) -> String {
        let exp_min = (self.exp.saturating_sub(V2_EPOCH) / 60).min(0xFF_FFFF);
        let code = match &self.relay {
            None => 0,
            Some(r) => KNOWN_RELAYS
                .iter()
                .position(|k| k == r)
                .map_or(RELAY_CUSTOM, |i| i as u8 + 1),
        };
        let mut b = vec![LINK_VERSION];
        b.extend(self.device);
        b.extend(self.secret);
        b.extend(&exp_min.to_be_bytes()[5..]);
        b.push(code << 5);
        if code == RELAY_CUSTOM {
            let r = self.relay.as_deref().unwrap();
            b.push(r.len() as u8);
            b.extend(r.as_bytes());
        }
        format!("{LINK_PREFIX}{}", b32_encode(&b))
    }

    /// Case-insensitive, tolerates surrounding whitespace.
    pub fn from_text(text: &str) -> Result<Self> {
        let text = text.trim();
        let head = text.get(..LINK_PREFIX.len()).ok_or(Error::UnknownVersion)?;
        if !head.eq_ignore_ascii_case(LINK_PREFIX) {
            return Err(Error::UnknownVersion);
        }
        let b = b32_decode(&text[LINK_PREFIX.len()..])?;
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8]> {
            let s = b.get(at..at + n).ok_or(Error::Decode)?;
            at += n;
            Ok(s)
        };
        if take(1)?[0] != LINK_VERSION {
            return Err(Error::UnknownVersion);
        }
        let device: [u8; 32] = take(32)?.try_into().unwrap();
        let secret: [u8; 16] = take(16)?.try_into().unwrap();
        let e = take(3)?;
        let exp = V2_EPOCH + 60 * ((e[0] as u64) << 16 | (e[1] as u64) << 8 | e[2] as u64);
        let flags = take(1)?[0];
        if flags & 31 != 0 {
            return Err(Error::Decode);
        }
        let relay = match flags >> 5 {
            0 => None,
            c @ 1..=4 => Some(KNOWN_RELAYS[c as usize - 1].to_string()),
            RELAY_CUSTOM => {
                let n = take(1)?[0] as usize;
                Some(
                    std::str::from_utf8(take(n)?)
                        .map_err(|_| Error::Decode)?
                        .to_string(),
                )
            }
            _ => return Err(Error::Decode),
        };
        if at != b.len() {
            return Err(Error::Decode);
        }
        Ok(Self {
            device,
            secret,
            exp,
            relay,
        })
    }
}

// ---- keys, proofs, confirmation code -----------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkRole {
    Scanner,
    Displayer,
}

impl LinkRole {
    fn label(self) -> &'static [u8] {
        match self {
            Self::Scanner => b"scanner",
            Self::Displayer => b"displayer",
        }
    }
}

/// `T = "tinline/link/v1" || E.device || N.device || secret`.
fn transcript(secret: &[u8; 16], e_dev: &[u8; 32], n_dev: &[u8; 32]) -> Vec<u8> {
    [T_PREFIX, e_dev, n_dev, secret].concat()
}

/// `K = HKDF-SHA256(ikm = secret, info = T)`.
fn link_key(secret: &[u8; 16], e_dev: &[u8; 32], n_dev: &[u8; 32]) -> [u8; 32] {
    let mut k = [0u8; 32];
    Hkdf::<Sha256>::new(None, secret)
        .expand(&transcript(secret, e_dev, n_dev), &mut k)
        .expect("32 bytes is a valid HKDF output");
    k
}

fn mac(key: &[u8; 32], parts: &[&[u8]]) -> Hmac<Sha256> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m
}

/// `HMAC(K, role || own_device)`: proves knowledge of the QR secret, bound to the device the
/// peer sees on the QUIC connection.
pub fn link_proof(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    role: LinkRole,
    own_device: &[u8; 32],
) -> [u8; 32] {
    let k = link_key(secret, e_dev, n_dev);
    mac(&k, &[role.label(), own_device]).finalize().into_bytes().into()
}

/// `remote_device` is the QUIC-authenticated key of the peer that claims `role`. Constant time.
pub fn verify_link_proof(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    role: LinkRole,
    remote_device: &[u8; 32],
    proof: &[u8],
) -> Result<()> {
    let k = link_key(secret, e_dev, n_dev);
    mac(&k, &[role.label(), remote_device])
        .verify_slice(proof)
        .map_err(|_| Error::BadSignature)
}

/// Six digits shown on both screens, e.g. `"482 913"`: 20 bits of `HMAC(K, "confirm")`.
pub fn confirm_code(secret: &[u8; 16], e_dev: &[u8; 32], n_dev: &[u8; 32]) -> String {
    let k = link_key(secret, e_dev, n_dev);
    let h = mac(&k, &[b"confirm"]).finalize().into_bytes();
    let n = ((h[0] as u32) << 12 | (h[1] as u32) << 4 | (h[2] as u32) >> 4) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

// ---- sealed grant ------------------------------------------------------------------

fn grant_cipher(secret: &[u8; 16], e: &[u8; 32], n: &[u8; 32]) -> Aes256Gcm {
    let k = link_key(secret, e, n);
    let mut gk = [0u8; 32];
    Hkdf::<Sha256>::new(None, &k)
        .expand(b"grant", &mut gk)
        .expect("32 bytes is a valid HKDF output");
    Aes256Gcm::new_from_slice(&gk).expect("32-byte key")
}

/// AES-256-GCM under a key derived from K ("grant"), AAD = T. Output is
/// `b64url(nonce(12) || ciphertext || tag)`.
pub fn seal_link_grant(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    plaintext: &[u8],
) -> String {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let aad = transcript(secret, e_dev, n_dev);
    let ct = grant_cipher(secret, e_dev, n_dev)
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .expect("AES-GCM encrypt");
    enc([&nonce[..], &ct].concat())
}

pub fn open_link_grant(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    sealed: &str,
) -> Result<Vec<u8>> {
    let data = dec(sealed)?;
    if data.len() < NONCE_LEN {
        return Err(Error::Decode);
    }
    let (nonce, ct) = data.split_at(NONCE_LEN);
    let aad = transcript(secret, e_dev, n_dev);
    grant_cipher(secret, e_dev, n_dev)
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
        .map_err(|_| Error::BadSignature)
}

// ---- messages (ALPN tinline/link/1) -------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryEntry {
    pub attestation: SignedAttestation,
    pub label: String,
    pub removed: bool,
}

/// Plaintext of `LinkMsg::LinkGrant`. The mnemonic is the identity secret: handle accordingly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkGrantBody {
    pub mnemonic: String,
    pub account_name: String,
    pub registry: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum LinkMsg {
    LinkHello { role: LinkRole, proof: String },
    LinkGrant { sealed: String },
    LinkDone { attestation: SignedAttestation, label: String },
    Unlinked,
    Reject { reason: String },
}

pub fn encode_link_frame(msg: &LinkMsg) -> Vec<u8> {
    encode_json_frame(msg)
}

pub fn decode_link_frame(buf: &[u8]) -> Result<Option<(LinkMsg, usize)>> {
    decode_json_frame(buf)
}

pub fn link_hello(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    role: LinkRole,
    own_device: &[u8; 32],
) -> LinkMsg {
    LinkMsg::LinkHello {
        role,
        proof: enc(link_proof(secret, e_dev, n_dev, role, own_device)),
    }
}

/// Checks a `LinkHello` from the peer: it must claim `expect_role`, and its proof must match.
pub fn accept_link_hello(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    expect_role: LinkRole,
    remote_device: &[u8; 32],
    msg: &LinkMsg,
) -> Result<()> {
    let LinkMsg::LinkHello { role, proof } = msg else {
        return Err(Error::UnexpectedMessage);
    };
    if *role != expect_role {
        return Err(Error::UnexpectedMessage);
    }
    verify_link_proof(secret, e_dev, n_dev, expect_role, remote_device, &dec(proof)?)
}

pub fn seal_link_grant_body(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    body: &LinkGrantBody,
) -> LinkMsg {
    let json = serde_json::to_vec(body).expect("body serializes");
    LinkMsg::LinkGrant {
        sealed: seal_link_grant(secret, e_dev, n_dev, &json),
    }
}

pub fn open_link_grant_body(
    secret: &[u8; 16],
    e_dev: &[u8; 32],
    n_dev: &[u8; 32],
    msg: &LinkMsg,
) -> Result<LinkGrantBody> {
    let LinkMsg::LinkGrant { sealed } = msg else {
        return Err(Error::UnexpectedMessage);
    };
    serde_json::from_slice(&open_link_grant(secret, e_dev, n_dev, sealed)?)
        .map_err(|_| Error::Decode)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedDevice {
    pub attestation: Attestation,
    /// Sanitized, at most `MAX_NAME_CHARS`.
    pub label: String,
}

/// E's check of N's `LinkDone`: a valid attestation by our DID for exactly the device the
/// transport authenticated.
pub fn accept_link_done(
    msg: &LinkMsg,
    my_did: &str,
    remote_device: [u8; 32],
) -> Result<LinkedDevice> {
    let LinkMsg::LinkDone { attestation, label } = msg else {
        return Err(Error::UnexpectedMessage);
    };
    let att = verify_attestation(attestation)?;
    if att.did != my_did {
        return Err(Error::WrongIssuer);
    }
    if att.device_key()? != remote_device {
        return Err(Error::DeviceMismatch);
    }
    Ok(LinkedDevice {
        attestation: att,
        label: sanitize_name(label),
    })
}
