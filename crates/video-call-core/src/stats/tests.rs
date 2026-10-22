use super::*;

#[test]
fn video_stats_keep_admission_receive_and_decode_distinct() {
    let mut st = VideoStats::default();
    st.bitrate_bps = 500_000;
    st.record_admitted(137);
    st.record_admitted(211);
    st.record_received(137);
    assert_eq!((st.admitted_payload_bytes, st.frames_sent), (348, 2));
    assert_eq!((st.received_payload_bytes, st.frames_received), (137, 1));
    assert_eq!(st.frames_decoded, 0);
    assert_eq!(st.bitrate_bps, 500_000, "target is not measured payload traffic");
}
