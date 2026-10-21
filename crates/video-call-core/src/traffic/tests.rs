use super::*;
#[test]
fn measured_rate_uses_elapsed_time_and_handles_startup_and_idle() {
    assert_eq!(payload_bps(125_000, 1.0), 1_000_000);
    assert_eq!(payload_bps(125_000, 2.0), 500_000);
    assert_eq!(payload_bps(0, 1.0), 0);
    assert_eq!(payload_bps(125_000, 0.001), 0);
    assert_eq!(payload_bps(125_000, f32::NAN), 0);
    assert_eq!(payload_bps(u64::MAX, 1.0), u32::MAX);
}
#[test]
fn counts_payload_bytes_and_units_not_target_rate() {
    let (mut bytes, mut units) = (0, 0);
    record(&mut bytes, &mut units, 137);
    record(&mut bytes, &mut units, 211);
    assert_eq!((bytes, units), (348, 2));
}
#[test]
fn zero_payload_does_not_invent_bytes() {
    let (mut bytes, mut units) = (0, 0);
    record(&mut bytes, &mut units, 0);
    assert_eq!((bytes, units), (0, 1));
}
#[test]
fn long_running_counts_saturate_never_wrap() {
    let (mut bytes, mut units) = (u64::MAX - 1, u64::MAX);
    record(&mut bytes, &mut units, 4);
    assert_eq!((bytes, units), (u64::MAX, u64::MAX));
}
#[test]
fn independent_directions_do_not_share_history() {
    let (mut send_bytes, mut sent) = (0, 0);
    let (mut recv_bytes, mut received) = (0, 0);
    record(&mut send_bytes, &mut sent, 130);
    record(&mut recv_bytes, &mut received, 80);
    assert_eq!((send_bytes, sent), (130, 1));
    assert_eq!((recv_bytes, received), (80, 1));
}
