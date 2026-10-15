use super::*;

const T0: u64 = 1_800_000_000;
const TTL: u64 = 600;
const GTTL: u64 = 86_400;

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

fn none() -> HashSet<String> {
    HashSet::new()
}

fn ticket(a: &Peer) -> ContactTicket {
    issue_contact_ticket(&a.id, a.dev, "alice", Some("https://relay".into()), T0, TTL)
}

/// Full add-contact: returns (alice's view of bob, bob's view of alice).
fn add_contact(a: &Peer, b: &Peer) -> (NewContact, NewContact) {
    let t = ticket(a);
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, "bob", T0 + 1, GTTL).unwrap();
    let (welcome, a_view) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0 + 2, &none(), GTTL).unwrap();
    let b_view = accept_contact_welcome(&b.id, &pending, &welcome, a.dev, T0 + 3).unwrap();
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
