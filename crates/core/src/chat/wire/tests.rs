use super::*;

#[test]
fn batch_signatures_bind_doc_and_update() {
    let secret = proto::new_device_secret();
    let public = proto::device_public(&secret);
    let sig = sign_batch(&secret, "dm/p/2026-10-07", b"update");
    assert!(verify_batch(&public, "dm/p/2026-10-07", b"update", &sig));
    assert!(!verify_batch(&public, "dm/p/2026-10-08", b"update", &sig));
    assert!(!verify_batch(&public, "dm/p/2026-10-07", b"updatf", &sig));
    let other = proto::device_public(&proto::new_device_secret());
    assert!(!verify_batch(&other, "dm/p/2026-10-07", b"update", &sig));
    assert!(!verify_batch(&public, "x", b"update", &sig[..10]));
}

#[test]
fn frames_encode() {
    let m = ChatMsg::Hello { shards: vec![("2026-10-07".into(), vec![1, 2, 3])] };
    let b = cryptography::codec::to_bytes(&m).unwrap();
    let back: ChatMsg = cryptography::codec::from_bytes(&b, usize::MAX).unwrap();
    assert!(matches!(back, ChatMsg::Hello { shards } if shards[0].1 == vec![1, 2, 3]));
}
