use super::*;

const DID: &str = "did:key:z6MkSyntheticOwnerForFetchLogTests";

#[test]
fn a_reason_is_logged_once_then_again_when_it_changes_or_after_the_repeat() {
    let mut log = FetchLog::default();
    let t0 = Instant::now();
    assert!(log.should_log(DID, "h1", "own 01234567: cannot connect", t0));
    // The 30 s kick loop: the same reason is not logged again.
    for i in 1..20 {
        assert!(!log.should_log(
            DID,
            "h1",
            "own 01234567: cannot connect",
            t0 + Duration::from_secs(30 * i)
        ));
    }
    assert!(
        log.should_log(
            DID,
            "h1",
            "own 01234567: refused",
            t0 + Duration::from_secs(30)
        ),
        "a new reason"
    );
    assert!(
        log.should_log(
            DID,
            "h1",
            "own 01234567: refused",
            t0 + Duration::from_secs(30) + REPEAT_EVERY
        ),
        "the repeat"
    );
}

#[test]
fn pictures_are_limited_separately_and_success_forgets() {
    let mut log = FetchLog::default();
    let t0 = Instant::now();
    assert!(log.should_log(DID, "h1", "x", t0));
    assert!(log.should_log(DID, "h2", "x", t0), "another picture");
    assert!(
        log.should_log("did:key:z6MkOther", "h1", "x", t0),
        "another owner"
    );
    assert!(log.succeeded(DID, "h1"));
    assert!(!log.succeeded(DID, "h1"));
    assert!(
        log.should_log(DID, "h1", "x", t0),
        "failing again after a success is new"
    );
}

#[test]
fn old_entries_are_forgotten() {
    let mut log = FetchLog::default();
    let t0 = Instant::now();
    assert!(log.should_log(DID, "h1", "x", t0));
    log.should_log(DID, "h2", "x", t0 + FORGET_AFTER);
    assert!(!log.last.contains_key(&(DID.to_string(), "h1".to_string())));
}

#[test]
fn network_error_text_never_reaches_the_log() {
    let e = crate::Error::Net("connect to 192.0.2.7:4433 failed".into());
    assert_eq!(class(&e), "transfer failed");
    assert_eq!(class(&crate::Error::Timeout), "stalled");
}
