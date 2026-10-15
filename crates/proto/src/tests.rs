use super::*;

/// Unix seconds (tickets, links); tokens and hellos count in ms from `M0`.
const T0: u64 = 1_800_000_000;
const M0: u64 = T0 * 1000;
/// Ticket lifetime, seconds.
const TTL: u64 = 600;
/// Token lifetime, ms.
const GTTL: u64 = 86_400_000;

struct Peer {
    id: Identity,
    secret: [u8; 32],
    dev: [u8; 32],
}

fn peer() -> Peer {
    let (id, _) = identity::generate();
    let secret = new_device_secret();
    Peer {
        id,
        dev: cryptography::signature::public_key(&secret),
        secret,
    }
}

/// An issuer's spent set of `redeemed_key(joiner, nonce)` entries.
struct Spent(HashSet<String>);

impl IssuanceStore for Spent {
    fn spent(&self, _: &str, nonce: &[u8], holder: &str) -> bool {
        self.0.contains(&redeemed_key(holder, &enc(nonce)))
    }
    fn cut_off_ms(&self, _: &str, _: domains::Domain) -> u64 {
        0
    }
}

/// A token checked by its holder: signature, issuer, holder, window.
fn verify_grant(g: &SignedGrant, iss: &str, holder: &str, now_ms: u64) -> Result<Grant> {
    g.redeem(&Expect { iss: Some(iss), holder, now_ms }, &Unrecorded)
}

fn ticket(a: &Peer) -> ContactTicket {
    issue_contact_ticket(&a.id, a.dev, "alice", Some("https://relay".into()), T0, TTL)
}

/// Full add-contact: returns (alice's view of bob, bob's view of alice).
fn add_contact(a: &Peer, b: &Peer) -> (NewContact, NewContact) {
    let t = ticket(a);
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, "bob", M0 + 1, GTTL).unwrap();
    let (welcome, a_view) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0 + 2, &Unrecorded, GTTL).unwrap();
    let b_view = accept_contact_welcome(&b.id, &pending, &welcome, a.dev, M0 + 3).unwrap();
    (a_view, b_view)
}

fn hello_parts(
    m: &Msg,
) -> (
    SignedAttestation,
    String,
    Signed<Ticket>,
    String,
    SignedGrant,
) {
    match m.clone() {
        Msg::ContactHello {
            relay: None,
            attestation,
            name,
            invite,
            binding,
            grant_for_you,
            ..
        } => (attestation, name, invite, binding, grant_for_you),
        _ => panic!(),
    }
}

/// The same bytes read as another statement type: what a cross-type replay looks like.
fn retype<T: Signable, U: Signable>(s: &Signed<T>) -> Signed<U> {
    Signed::from_bytes(&s.to_bytes()).unwrap()
}

fn dev() -> [u8; 32] {
    cryptography::signature::public_key(&new_device_secret())
}

mod devlist;
mod handshake;
mod link;
mod notices;
mod ticket;
mod wire;
