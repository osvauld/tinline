use serde::{Deserialize, Serialize};

use super::*;

const PROBE: Domain = b"test/signed/probe";
const PROBE_KEY: Domain = b"test/signed/probe-key";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Probe {
    did: String,
    n: u64,
    text: String,
}

impl Signable for Probe {
    const DOMAIN: Domain = PROBE;
    const MAX_BODY: usize = 128;
    fn signer(&self) -> Signer<'_> {
        Signer::Did(&self.did)
    }
    fn check(&self) -> Result<()> {
        if self.text == "bad" { Err(Error::Decode) } else { Ok(()) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KeyProbe {
    key: [u8; 32],
    n: u32,
}

impl Signable for KeyProbe {
    const DOMAIN: Domain = PROBE_KEY;
    const MAX_BODY: usize = 64;
    fn signer(&self) -> Signer<'_> {
        Signer::Key(&self.key)
    }
}

fn probe(id: &Identity) -> Probe {
    Probe { did: id.did().to_string(), n: 300, text: "hi".into() }
}

#[test]
fn table_for_probe_types() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    conformance::check(&probe(&a), &a, &b);

    let secret = [7u8; 32];
    let other = [8u8; 32];
    let kp = KeyProbe { key: signature::public_key(&secret), n: 5 };
    conformance::check(&kp, &SecretKey(&secret), &SecretKey(&other));
    // An identity signs a raw-key statement naming its own account key.
    let mine = KeyProbe { key: a.signing_public_key(), n: 1 };
    conformance::check(&mine, &a, &b);
    assert_eq!(Signed::sign(&mine, &a).unwrap().open_from(a.did()).unwrap(), mine);
}

#[test]
fn overlong_varint_is_refused_even_when_signed() {
    let (a, _) = identity::generate();
    let p = Probe { n: 1, ..probe(&a) };
    let good = codec::to_bytes(&p).unwrap();
    // Re-spell `n = 1` as the two-byte varint 0x81 0x00.
    let at = 1 + p.did.len();
    assert_eq!(good[at], 1);
    let body = [&good[..at], &[0x81, 0x00], &good[at + 1..]].concat();
    let sig = a.sign(&sign::input(PROBE, &[&body]));
    assert_eq!(Signed::<Probe>::from_parts(body, sig).open(), Err(Error::Decode));
}

#[test]
fn size_cap_and_shape_check_on_sign_and_open() {
    let (a, _) = identity::generate();
    let big = Probe { text: "x".repeat(200), ..probe(&a) };
    assert_eq!(Signed::sign(&big, &a).err(), Some(Error::TooLarge));
    let bad = Probe { text: "bad".into(), ..probe(&a) };
    assert_eq!(Signed::sign(&bad, &a).err(), Some(Error::Decode));
    let body = codec::to_bytes(&bad).unwrap();
    let sig = a.sign(&sign::input(PROBE, &[&body]));
    assert_eq!(Signed::<Probe>::from_parts(body, sig).open(), Err(Error::Decode));
}

#[test]
fn peek_and_id() {
    let (a, _) = identity::generate();
    let s = Signed::sign(&probe(&a), &a).unwrap();
    assert_eq!(s.peek().unwrap(), probe(&a));
    assert_eq!(s.peek_signer().unwrap(), SignerId::Did(a.did().to_string()));
    assert_eq!(s.id(), hash::content(PROBE, &s.to_bytes()));
    assert_eq!(s.len(), s.to_bytes().len());
}

#[test]
fn serde_is_a_string_in_json_and_bytes_in_postcard() {
    let (a, _) = identity::generate();
    let s = Signed::sign(&probe(&a), &a).unwrap();
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.starts_with('"'));
    assert_eq!(serde_json::from_str::<Signed<Probe>>(&json).unwrap(), s);
    let bin = codec::to_bytes(&s).unwrap();
    assert_eq!(codec::from_bytes::<Signed<Probe>>(&bin, 1024).unwrap(), s);
}

#[test]
fn detached_round_trip_and_tamper() {
    let (a, _) = identity::generate();
    let sig = sign_detached(&a, PROBE, &[b"x", b"y"]);
    assert_eq!(verify_detached(Signer::Did(a.did()), PROBE, &[b"x", b"y"], &sig), Ok(()));
    assert_eq!(verify_detached(Signer::Did(a.did()), PROBE, &[b"xy"], &sig), Err(Error::BadSignature));
    assert_eq!(verify_detached(Signer::Did(a.did()), PROBE_KEY, &[b"x", b"y"], &sig), Err(Error::BadSignature));
    let key = a.signing_public_key();
    assert_eq!(verify_detached(Signer::Key(&key), PROBE, &[b"x", b"y"], &sig), Ok(()));
}
