use super::*;

// KIND: unit (v2) — the varint and chunk codecs round-trip.
#[test]
fn codecs_round_trip() {
    let l = vec![3000, 3001, 3005, 3130, 70_000, 4_000_000];
    assert_eq!(decode_list(&encode_list(&l, 2048), 2048).unwrap(), l);
    let r = |id: &str, at: i64| DocRow { src: "g.a".into(), conv: "a".into(), day: "2026-01-02".into(), id: id.into(), at, author: "did:x".into(), text: format!("héllo {id} 日本") };
    let rows = [r("1", 5), r("2", -3), r("3", 1 << 40)];
    let refs: Vec<(u64, &DocRow)> = rows.iter().enumerate().map(|(i, r)| (i as u64 + 9, r)).collect();
    let back = decode_chunk(&encode_chunk(&refs)).unwrap();
    assert_eq!(back.len(), 3);
    for ((h, got), (want_h, want)) in back.iter().zip(&refs) {
        assert_eq!((h, got), (want_h, *want));
    }
    assert!(decode_chunk(&[9, 9, 9]).is_none());
}
