use super::*;
#[test]
fn inaudible_idle_and_invalid_bytes_do_not_drive_a_field() {
    assert!(!candidate(false, false, 10, 0, 0, 0, 0));
    assert!(!candidate(true, true, 10, 0, 0, 0, 0));
    assert!(!candidate(true, false, 255, 0, 0, 0, 0));
}
#[test]
fn plc_or_missing_packet_metadata_is_not_promoted_to_fresh_input() {
    assert!(!candidate(true, false, 10, 0, 1, 0, 0));
    assert!(!candidate(true, false, 10, 0, 0, 0, 1));
    assert!(candidate(true, false, 30, 4, 4, 2, 2));
}
