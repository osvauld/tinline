//! Contact tickets: signed invite (v1 JSON) and the compact v2 text form.

use super::*;

// ---- contact ticket ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InviteClaim {
    pub v: u8,
    pub iss: String,
    pub nonce: String,
    pub iat: u64,
    pub exp: u64,
    pub device: String,
    pub name: String,
    pub relay: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactTicket {
    pub v: u8,
    pub did: String,
    pub name: String,
    pub device: String,
    pub relay: Option<String>,
    pub invite: SignedBlob,
}

pub(crate) fn open_invite(blob: &SignedBlob) -> Result<InviteClaim> {
    if is_v2_payload(blob)? {
        return open_invite_v2(blob);
    }
    let claim: InviteClaim = open_blob(blob, INVITE_DOMAIN, |c: &InviteClaim| &c.iss)?;
    check_version(claim.v)?;
    if dec(&claim.nonce)?.len() != 16 {
        return Err(Error::Decode);
    }
    Ok(claim)
}

// ---- compact v2 invite -------------------------------------------------------------
//
// Binary body (what the QR carries, after the `OSVC2:` prefix, base32 uppercase, no pad):
//   signing_pk[32] | device[32] | nonce[6] | exp[3] | flags[1] | name[len] | relay_ext | sig[64]
// - exp: minutes since 2026-01-01T00:00:00Z, u24 big endian (rounded up when issuing).
// - flags: high 3 bits relay code (0 none, 1..=4 known n0 relays, 7 custom with a trailing
//   u8 length + URL), low 5 bits name length in bytes (so names are capped at 31 bytes).
// - sig: Ed25519 by the DID key over `INVITE_V2_DOMAIN || 0x02 || everything before sig`.
// The issuer DID is `did_from_public_key(signing_pk)`; `iat` is not carried (decodes as 0).
// Inside `SignedBlob` the payload is `b64url(0x02 || body-before-sig)`; a v1 payload is JSON
// and starts with `{`, so the first byte tells the versions apart.

pub(crate) const V2_BYTE: u8 = 2;
pub(crate) const V2_EPOCH: u64 = 1_767_225_600;
pub(crate) const V2_NONCE_LEN: usize = 6;
pub(crate) const V2_MAX_NAME: usize = 31;
pub(crate) const KNOWN_RELAYS: [&str; 4] = [
    "https://use1-1.relay.n0.iroh.link./",
    "https://usw1-1.relay.n0.iroh.link./",
    "https://euc1-1.relay.n0.iroh.link./",
    "https://aps1-1.relay.n0.iroh.link./",
];
pub(crate) const RELAY_CUSTOM: u8 = 7;
pub(crate) const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub(crate) fn b32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[(acc >> bits & 31) as usize] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(B32[(acc << (5 - bits) & 31) as usize] as char);
    }
    out
}

pub(crate) fn b32_decode(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = B32
            .iter()
            .position(|&x| x == c.to_ascii_uppercase())
            .ok_or(Error::Decode)? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Leftover bits are padding and must be zero (canonical encoding).
    if bits >= 5 || acc != 0 {
        return Err(Error::Decode);
    }
    Ok(out)
}

pub(crate) fn is_v2_payload(blob: &SignedBlob) -> Result<bool> {
    Ok(dec(&blob.payload)?.first().copied() == Some(V2_BYTE))
}

pub(crate) struct V2Body {
    pk: [u8; 32],
    device: [u8; 32],
    nonce: [u8; V2_NONCE_LEN],
    exp: u64,
    name: String,
    relay: Option<String>,
    /// Bytes consumed from the body, i.e. where the signature starts.
    len: usize,
}

pub(crate) fn parse_v2_body(b: &[u8]) -> Result<V2Body> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8]> {
        let s = b.get(at..at + n).ok_or(Error::Decode)?;
        at += n;
        Ok(s)
    };
    let pk: [u8; 32] = take(32)?.try_into().unwrap();
    let device: [u8; 32] = take(32)?.try_into().unwrap();
    let nonce: [u8; V2_NONCE_LEN] = take(V2_NONCE_LEN)?.try_into().unwrap();
    let e = take(3)?;
    let exp = V2_EPOCH + 60 * ((e[0] as u64) << 16 | (e[1] as u64) << 8 | e[2] as u64);
    let flags = take(1)?[0];
    let name = sanitize_name(
        std::str::from_utf8(take((flags & 31) as usize)?).map_err(|_| Error::Decode)?,
    );
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
    Ok(V2Body {
        pk,
        device,
        nonce,
        exp,
        name,
        relay,
        len: at,
    })
}

pub(crate) fn open_invite_v2(blob: &SignedBlob) -> Result<InviteClaim> {
    let payload = dec(&blob.payload)?;
    let sig: [u8; 64] = dec(&blob.signature)?
        .try_into()
        .map_err(|_| Error::Decode)?;
    let body = parse_v2_body(&payload[1..])?;
    if body.len + 1 != payload.len() {
        return Err(Error::Decode);
    }
    if !verify(&body.pk, &[INVITE_V2_DOMAIN, &payload].concat(), &sig) {
        return Err(Error::BadSignature);
    }
    Ok(InviteClaim {
        v: 2,
        iss: identity::did_from_public_key(&body.pk),
        nonce: enc(body.nonce),
        iat: 0,
        exp: body.exp,
        device: enc(body.device),
        name: body.name,
        relay: body.relay,
    })
}

impl ContactTicket {
    /// Text form. A ticket we issue is v2 (`OSVC2:` + base32, QR-friendly); a ticket parsed
    /// from `osvc1.` text can't be re-signed, so it keeps its v1 form.
    pub fn to_text(&self) -> String {
        if let Ok(true) = is_v2_payload(&self.invite)
            && let (Ok(payload), Ok(sig)) = (dec(&self.invite.payload), dec(&self.invite.signature))
        {
            let bytes = [&payload[1..], &sig[..]].concat();
            return format!("{TICKET_PREFIX_V2}{}", b32_encode(&bytes));
        }
        let json = serde_json::to_vec(self).expect("ticket serializes");
        format!("{TICKET_PREFIX}{}", enc(json))
    }

    /// Accepts v1 (`osvc1.`) and v2 (`OSVC2:`, case-insensitive), tolerating whitespace.
    pub fn from_text(text: &str) -> Result<Self> {
        let text = text.trim();
        if let Some(head) = text.get(..TICKET_PREFIX_V2.len())
            && head.eq_ignore_ascii_case(TICKET_PREFIX_V2)
        {
            return Self::from_v2(&text[TICKET_PREFIX_V2.len()..]);
        }
        let body = text
            .strip_prefix(TICKET_PREFIX)
            .ok_or(Error::UnknownVersion)?;
        let t: Self = serde_json::from_slice(&dec(body)?).map_err(|_| Error::Decode)?;
        check_version(t.v)?;
        Ok(t)
    }

    fn from_v2(text: &str) -> Result<Self> {
        let bytes = b32_decode(text)?;
        let split = bytes.len().checked_sub(64).ok_or(Error::Decode)?;
        let payload = [&[V2_BYTE][..], &bytes[..split]].concat();
        let body = parse_v2_body(&payload[1..])?;
        if body.len + 1 != payload.len() {
            return Err(Error::Decode);
        }
        Ok(Self {
            v: 2,
            did: identity::did_from_public_key(&body.pk),
            name: body.name,
            device: enc(body.device),
            relay: body.relay,
            invite: SignedBlob {
                payload: enc(&payload),
                signature: enc(&bytes[split..]),
            },
        })
    }

    /// The visible fields are only for display before scanning; the signed claim is what
    /// counts, so any disagreement is an error rather than silently preferring one.
    pub fn verify(&self, now: u64) -> Result<InviteClaim> {
        if self.v != 1 && self.v != 2 {
            return Err(Error::UnknownVersion);
        }
        let claim = open_invite(&self.invite)?;
        if claim.v != self.v {
            return Err(Error::TicketMismatch);
        }
        if claim.iss != self.did
            || claim.device != self.device
            || claim.name != self.name
            || claim.relay != self.relay
        {
            return Err(Error::TicketMismatch);
        }
        if now >= claim.exp {
            return Err(Error::Expired);
        }
        Ok(claim)
    }
}

pub fn issue_contact_ticket(
    id: &Identity,
    device: [u8; 32],
    name: &str,
    relay: Option<String>,
    now: u64,
    ttl_secs: u64,
) -> ContactTicket {
    // Fit the compact layout: name capped at a char boundary, expiry rounded up to a minute.
    let name = sanitize_name(name);
    let mut end = name.len().min(V2_MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let name = &name[..end];
    let relay = relay.filter(|r| r.len() <= 255);
    let exp_min = now
        .saturating_add(ttl_secs)
        .saturating_sub(V2_EPOCH)
        .div_ceil(60)
        .min(0xFF_FFFF);
    let mut nonce = [0u8; V2_NONCE_LEN];
    cryptography::rng::fill(&mut nonce);
    let code = match &relay {
        None => 0,
        Some(r) => KNOWN_RELAYS
            .iter()
            .position(|k| k == r)
            .map_or(RELAY_CUSTOM, |i| i as u8 + 1),
    };
    let mut payload = vec![V2_BYTE];
    payload.extend(id.signing_public_key());
    payload.extend(device);
    payload.extend(nonce);
    payload.extend(&exp_min.to_be_bytes()[5..]);
    payload.push(code << 5 | name.len() as u8);
    payload.extend(name.as_bytes());
    if code == RELAY_CUSTOM {
        let r = relay.as_deref().unwrap();
        payload.push(r.len() as u8);
        payload.extend(r.as_bytes());
    }
    let sig = id.sign(&[INVITE_V2_DOMAIN, &payload].concat());
    ContactTicket {
        v: 2,
        did: id.did().to_string(),
        name: name.to_string(),
        device: enc(device),
        relay,
        invite: SignedBlob {
            payload: enc(&payload),
            signature: enc(sig),
        },
    }
}
