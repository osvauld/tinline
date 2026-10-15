use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::*;
use crate::Cap;

const T: u64 = 1_800_000_000_000;
const DAY: u64 = 24 * 3600 * 1000;

/// An issuer's memory: spent (iss, nonce, holder) and per-holder cut-offs.
#[derive(Default)]
struct Mem {
    spent: RefCell<HashSet<(String, Vec<u8>, String)>>,
    cut: HashMap<String, u64>,
}

impl IssuanceStore for Mem {
    fn spent(&self, iss: &str, nonce: &[u8], holder: &str) -> bool {
        self.spent.borrow().contains(&(iss.to_string(), nonce.to_vec(), holder.to_string()))
    }
    fn cut_off_ms(&self, holder: &str, _: Domain) -> u64 {
        self.cut.get(holder).copied().unwrap_or(0)
    }
}

fn token(iss: &Identity, holder: &str) -> Signed<Issued<Cap>> {
    issue(iss, Subject::Did(holder.into()), T, DAY, Cap::Message).unwrap()
}

fn at<'a>(iss: &'a str, holder: &'a str, now_ms: u64) -> Expect<'a> {
    Expect { iss: Some(iss), holder, now_ms }
}

#[test]
fn a_good_statement_redeems_once_spent_it_does_not() {
    let (a, _) = identity::generate();
    let t = token(&a, "did:key:zB");
    let store = Mem::default();
    let got = t.redeem(&at(a.did(), "did:key:zB", T + 1), &store).unwrap();
    assert_eq!((got.claim, got.iat_ms, got.exp_ms), (Cap::Message, T, T + DAY));
    store.spent.borrow_mut().insert((a.did().to_string(), got.nonce.to_vec(), "did:key:zB".into()));
    assert_eq!(t.redeem(&at(a.did(), "did:key:zB", T + 1), &store).err(), Some(Error::Replayed));
}

#[test]
fn a_holder_cut_off_takes_back_what_was_issued_at_or_before_it() {
    let (a, _) = identity::generate();
    let t = token(&a, "did:key:zB");
    let mut store = Mem::default();
    store.cut.insert("did:key:zB".into(), T);
    assert_eq!(t.redeem(&at(a.did(), "did:key:zB", T + 1), &store).err(), Some(Error::Revoked));
    store.cut.insert("did:key:zB".into(), T - 1);
    assert!(t.redeem(&at(a.did(), "did:key:zB", T + 1), &store).is_ok());
    // Another holder's cut-off does not touch it.
    store.cut.insert("did:key:zC".into(), u64::MAX);
    assert!(t.redeem(&at(a.did(), "did:key:zB", T + 1), &store).is_ok());
}

#[test]
fn the_validity_window_is_iat_minus_skew_to_exp_exclusive() {
    let (a, _) = identity::generate();
    let t = token(&a, "did:key:zB");
    let s = Unrecorded;
    assert_eq!(t.redeem(&at(a.did(), "did:key:zB", T + DAY), &s).err(), Some(Error::Expired));
    assert!(t.redeem(&at(a.did(), "did:key:zB", T + DAY - 1), &s).is_ok());
    assert!(t.redeem(&at(a.did(), "did:key:zB", T - ISSUE_SKEW_MS), &s).is_ok());
    assert_eq!(t.redeem(&at(a.did(), "did:key:zB", T - ISSUE_SKEW_MS - 1), &s).err(), Some(Error::NotYetValid));
}

#[test]
fn wrong_holder_and_wrong_issuer_are_refused() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let t = token(&a, "did:key:zB");
    assert_eq!(t.redeem(&at(a.did(), "did:key:zC", T), &Unrecorded).err(), Some(Error::WrongHolder));
    assert_eq!(t.redeem(&at(b.did(), "did:key:zB", T), &Unrecorded).err(), Some(Error::WrongIssuer));
    // No expected issuer: any signer, as long as it signed.
    assert!(t.redeem(&Expect { iss: None, holder: "did:key:zB", now_ms: T }, &Unrecorded).is_ok());
}

#[test]
fn a_bearer_statement_redeems_for_anyone_until_it_expires() {
    let (a, _) = identity::generate();
    let t = issue(&a, Subject::Anyone, T, DAY, Cap::Call).unwrap();
    for holder in ["did:key:zB", "did:key:zC"] {
        assert!(t.redeem(&at(a.did(), holder, T + 1), &Unrecorded).is_ok());
    }
    assert_eq!(t.redeem(&at(a.did(), "did:key:zB", T + DAY), &Unrecorded).err(), Some(Error::Expired));
}

#[test]
fn shape_bounds_the_lifetime() {
    let (a, _) = identity::generate();
    assert!(issue(&a, Subject::Anyone, T, 0, Cap::Call).is_err(), "exp must be after iat");
    assert!(issue(&a, Subject::Anyone, T, <Cap as IssuedClaim>::MAX_TTL_MS + 1, Cap::Call).is_err());
    assert!(issue(&a, Subject::Anyone, T, <Cap as IssuedClaim>::MAX_TTL_MS, Cap::Call).is_ok());
}

#[test]
fn conformance_rows() {
    let (a, _) = identity::generate();
    let (b, _) = identity::generate();
    let grant = token(&a, b.did()).open().unwrap();
    crate::signed::conformance::check(&grant, &a, &b);
    let bearer = issue(&a, Subject::Anyone, T, DAY, Cap::Call).unwrap().open().unwrap();
    crate::signed::conformance::check(&bearer, &a, &b);
}
