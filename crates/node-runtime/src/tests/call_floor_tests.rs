use crate::*;
use std::sync::Arc;

const A: u64 = 1;
const B: u64 = 2;

#[test]
fn the_floor_is_free_until_taken_and_busy_for_others_while_held() {
    let f = CallFloor::default();
    assert!(!f.held_by_other(A) && !f.held_by_other(B));
    assert!(f.take(A));
    assert!(!f.held_by_other(A), "the holder is not 'other'");
    assert!(f.held_by_other(B));
    assert!(!f.take(B), "busy while A holds it");
    assert!(f.held_by_other(B), "a failed take changes nothing");
}

#[test]
fn taking_again_by_the_holder_succeeds_and_one_release_frees_it() {
    let f = CallFloor::default();
    assert!(f.take(A));
    assert!(f.take(A), "a waiting call on the holder's slot is the same slot");
    f.release(A);
    assert!(!f.held_by_other(B));
    assert!(f.take(B));
}

#[test]
fn only_the_holder_can_release() {
    let f = CallFloor::default();
    assert!(f.take(A));
    f.release(B);
    assert!(f.held_by_other(B), "B's release is a no-op");
    f.release(A);
    f.release(A);
    assert!(!f.held_by_other(B), "released, and a second release is harmless");
}

#[test]
fn the_floor_is_free_again_after_a_failed_start() {
    // `begin_call` takes the floor, then refuses a group call in progress: it gives it back.
    let f = CallFloor::default();
    let begin = |slot: u64, in_group_call: bool| -> Result<(), Error> {
        if !f.take(slot) {
            return Err(Error::InCall);
        }
        if in_group_call {
            f.release(slot);
            return Err(Error::InCall);
        }
        Ok(())
    };
    assert!(matches!(begin(A, true), Err(Error::InCall)));
    assert!(begin(B, false).is_ok(), "A's refused start did not keep the floor");
    assert!(matches!(begin(A, false), Err(Error::InCall)), "B holds it now");
}

#[test]
fn slots_sharing_one_floor_see_each_other() {
    let shared: Arc<CallFloor> = Default::default();
    let (x, y) = (shared.clone(), shared.clone());
    assert!(x.take(A));
    assert!(y.held_by_other(B));
    x.release(A);
    assert!(y.take(B));
    assert!(x.held_by_other(A));
}
