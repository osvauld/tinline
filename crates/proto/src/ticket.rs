//! Contact tickets: a signed invite, compact enough for a QR code (`OSVC2:` + base32).

use super::*;

/// What the scanner learns from a valid ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteClaim {
    pub iss: String,
    /// base64url of the ticket nonce; with the joiner DID it keys the redeemed set.
    pub nonce: String,
    /// Unix seconds.
    pub exp: u64,
    pub device: [u8; 32],
    pub name: String,
    pub relay: Option<String>,
}

pub(crate) const TICKET_EPOCH: u64 = 1_767_225_600;
pub(crate) const TICKET_NONCE_LEN: usize = 6;
pub(crate) const MAX_TICKET_NAME: usize = 31;
pub(crate) const MAX_TICKET_RELAY: usize = 255;
pub(crate) const KNOWN_RELAYS: [&str; 4] = [
    "https://use1-1.relay.n0.iroh.link./",
    "https://usw1-1.relay.n0.iroh.link./",
    "https://euc1-1.relay.n0.iroh.link./",
    "https://aps1-1.relay.n0.iroh.link./",
];
pub(crate) const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// A relay hint, with the n0 relays as one-byte codes so a typical QR stays short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TicketRelay {
    None,
    Known(u8),
    Custom(String),
}

impl TicketRelay {
    fn of(relay: Option<&str>) -> Self {
        match relay {
            None => TicketRelay::None,
            Some(r) => match KNOWN_RELAYS.iter().position(|k| *k == r) {
                Some(i) => TicketRelay::Known(i as u8),
                None => TicketRelay::Custom(r.to_string()),
            },
        }
    }

    fn url(&self) -> Option<String> {
        match self {
            TicketRelay::None => None,
            TicketRelay::Known(i) => KNOWN_RELAYS.get(*i as usize).map(|s| s.to_string()),
            TicketRelay::Custom(r) => Some(r.clone()),
        }
    }
}

/// The signed ticket body. Signed by the DID key it carries (`pk`), so the issuer DID needs no
/// room of its own: it is `did_from_public_key(pk)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub pk: [u8; 32],
    pub device: [u8; 32],
    pub nonce: [u8; TICKET_NONCE_LEN],
    /// Minutes since 2026-01-01T00:00:00Z, rounded up when issuing.
    pub exp_min: u32,
    pub name: String,
    pub relay: TicketRelay,
}

impl Signable for Ticket {
    const DOMAIN: domains::Domain = domains::INVITE_V2;
    const MAX_BODY: usize =
        32 + 32 + TICKET_NONCE_LEN + 5 + 1 + MAX_TICKET_NAME + 1 + 2 + MAX_TICKET_RELAY;
    fn signer(&self) -> Signer<'_> {
        Signer::Key(&self.pk)
    }
    fn check(&self) -> Result<()> {
        let relay_ok = match &self.relay {
            TicketRelay::None => true,
            TicketRelay::Known(i) => (*i as usize) < KNOWN_RELAYS.len(),
            TicketRelay::Custom(r) => !r.is_empty() && r.len() <= MAX_TICKET_RELAY,
        };
        if self.name.len() > MAX_TICKET_NAME || !relay_ok {
            return Err(Error::Decode);
        }
        Ok(())
    }
}

impl Ticket {
    pub fn exp(&self) -> u64 {
        TICKET_EPOCH + 60 * self.exp_min as u64
    }

    fn claim(&self) -> InviteClaim {
        InviteClaim {
            iss: identity::did_from_public_key(&self.pk),
            nonce: enc(self.nonce),
            exp: self.exp(),
            device: self.device,
            name: sanitize_name(&self.name),
            relay: self.relay.url(),
        }
    }
}

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

/// Checks the ticket signature, nothing else (expiry and who it is for are the caller's).
pub(crate) fn open_invite(t: &Signed<Ticket>) -> Result<InviteClaim> {
    Ok(t.open()?.claim())
}

/// A contact ticket: the signed invite plus its visible fields, read from the body for display
/// before verifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactTicket {
    pub did: String,
    pub name: String,
    pub device: [u8; 32],
    pub relay: Option<String>,
    pub invite: Signed<Ticket>,
}

impl ContactTicket {
    /// `OSVC2:` + base32 of the signed bytes.
    pub fn to_text(&self) -> String {
        format!("{TICKET_PREFIX_V2}{}", b32_encode(&self.invite.to_bytes()))
    }

    /// Case-insensitive prefix, tolerating surrounding whitespace.
    pub fn from_text(text: &str) -> Result<Self> {
        let text = text.trim();
        let head = text
            .get(..TICKET_PREFIX_V2.len())
            .ok_or(Error::UnknownVersion)?;
        if !head.eq_ignore_ascii_case(TICKET_PREFIX_V2) {
            return Err(Error::UnknownVersion);
        }
        let invite = Signed::<Ticket>::from_bytes(&b32_decode(&text[TICKET_PREFIX_V2.len()..])?)?;
        Ok(Self::of(invite)?)
    }

    fn of(invite: Signed<Ticket>) -> Result<Self> {
        let c = invite.peek()?.claim();
        Ok(Self {
            did: c.iss,
            name: c.name,
            device: c.device,
            relay: c.relay,
            invite,
        })
    }

    /// The visible fields are only for display before scanning; the signed claim is what
    /// counts, so any disagreement is an error rather than silently preferring one.
    pub fn verify(&self, now: u64) -> Result<InviteClaim> {
        let claim = open_invite(&self.invite)?;
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
    let mut end = name.len().min(MAX_TICKET_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let exp_min = now
        .saturating_add(ttl_secs)
        .saturating_sub(TICKET_EPOCH)
        .div_ceil(60)
        .min(u32::MAX as u64) as u32;
    let mut nonce = [0u8; TICKET_NONCE_LEN];
    cryptography::rng::fill(&mut nonce);
    let relay = relay.filter(|r| !r.is_empty() && r.len() <= MAX_TICKET_RELAY);
    let body = Ticket {
        pk: id.signing_public_key(),
        device,
        nonce,
        exp_min,
        name: name[..end].to_string(),
        relay: TicketRelay::of(relay.as_deref()),
    };
    let invite = Signed::sign(&body, id).expect("own ticket signs");
    ContactTicket::of(invite).expect("own ticket reads")
}
