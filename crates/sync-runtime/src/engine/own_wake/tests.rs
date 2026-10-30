use super::*;

fn marker(bytes: Vec<u8>) -> (String, Vec<u8>) {
    (CAPABILITY.into(), bytes)
}

#[test]
fn an_older_peer_without_the_marker_never_negotiates_it() {
    assert!(!supported(&[]));
    assert!(!supported(&[(
        group_history::CAPABILITY.into(),
        VersionVector::default().encode()
    )]));
    assert!(!supported(&[marker(vec![255])]), "a garbled marker");
    assert!(supported(&[marker(VersionVector::default().encode())]));
}

#[test]
fn nothing_is_sent_before_negotiation() {
    let mut w = OwnWake::default();
    assert!(w.due(Some(("tok".into(), 5))).is_none());
}

#[test]
fn the_token_is_sent_once_and_again_only_when_it_changes() {
    let mut w = OwnWake {
        enabled: true,
        ..Default::default()
    };
    assert!(
        matches!(w.due(None), Some(SelfMsg::OwnWake { token: None, .. })),
        "no token: say so, the peer drops an old one"
    );
    assert!(w.due(None).is_none());
    let first = w.due(Some(("tok".into(), 5)));
    assert!(
        matches!(first, Some(SelfMsg::OwnWake { token: Some(ref t), exp_ms: 5 }) if t == "tok")
    );
    assert!(w.due(Some(("tok".into(), 5))).is_none(), "unchanged");
    assert!(w.due(Some(("tok2".into(), 9))).is_some(), "renewed");
    assert!(w.due(None).is_some(), "push turned off");
}

#[test]
fn the_new_variant_round_trips_and_keeps_older_discriminants() {
    // Appended: an older variant still encodes as it did (its discriminant is unchanged).
    let old = cryptography::codec::to_bytes(&SelfMsg::Unlinked).unwrap();
    assert_eq!(old, vec![3], "Unlinked keeps discriminant 3");
    let m = SelfMsg::OwnWake {
        token: Some("tok".into()),
        exp_ms: 7,
    };
    let bytes = cryptography::codec::to_bytes(&m).unwrap();
    assert_eq!(bytes[0], 15, "appended after GroupReceiptPage (14)");
    let back: SelfMsg = cryptography::codec::from_bytes(&bytes, crate::frames::MAX_FRAME).unwrap();
    assert!(matches!(back, SelfMsg::OwnWake { token: Some(t), exp_ms: 7 } if t == "tok"));
}
