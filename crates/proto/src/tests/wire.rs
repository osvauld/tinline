//! Wire frames, name sanitising and message compatibility.

use super::*;

#[test]
fn frame_roundtrip_all_variants() {
    let a = peer();
    let g = issue_grant(&a.id, a.id.did(), T0, GTTL);
    let msgs = [
        Msg::Ringing,
        Msg::Busy,
        Msg::Hangup,
        Msg::Reject {
            reason: "no".into(),
        },
        Msg::Decline {
            reason: "later".into(),
        },
        Msg::Accept {
            renewed_grant: None,
            devices: None,
            video: None,
        },
        Msg::Accept {
            renewed_grant: Some(g),
            devices: None,
            video: Some(VideoOffer {
                max_pixels: 1280 * 720,
            }),
        },
        Msg::Video { camera: true },
        Msg::Cancel {
            reason: CancelReason::AnsweredElsewhere,
        },
    ];
    for m in msgs {
        let f = encode_frame(&m);
        let (back, n) = decode_frame(&f).unwrap().unwrap();
        assert_eq!((back, n), (m, f.len()));
    }
}

#[test]
fn frame_split_across_reads_and_back_to_back() {
    let m1 = Msg::Reject { reason: "x".into() };
    let m2 = Msg::Hangup;
    let mut wire = encode_frame(&m1);
    let first_len = wire.len();
    wire.extend(encode_frame(&m2));
    // Every prefix shorter than the first frame needs more bytes.
    for cut in 0..first_len {
        assert_eq!(decode_frame(&wire[..cut]).unwrap(), None, "cut {cut}");
    }
    let (got1, used) = decode_frame(&wire).unwrap().unwrap();
    assert_eq!((got1, used), (m1, first_len));
    let (got2, _) = decode_frame(&wire[used..]).unwrap().unwrap();
    assert_eq!(got2, m2);
}

#[test]
fn oversized_and_garbage_frames() {
    // Rejected from the header alone, before the body arrives.
    let head = ((MAX_FRAME + 1) as u32).to_be_bytes();
    assert_eq!(decode_frame(&head), Err(Error::FrameTooLarge));
    assert_eq!(
        decode_frame(&u32::MAX.to_be_bytes()),
        Err(Error::FrameTooLarge)
    );
    let mut f = 5u32.to_be_bytes().to_vec();
    f.extend(b"nojso");
    assert_eq!(decode_frame(&f), Err(Error::Decode));
}

#[test]
fn sanitize_name_strips_and_caps() {
    assert_eq!(sanitize_name("bo\u{202E}b\u{0}\n\u{2066}x\u{2069}"), "bobx");
    assert_eq!(sanitize_name("  Zoë  "), "Zoë");
    let long = "é".repeat(200);
    assert_eq!(sanitize_name(&long).chars().count(), MAX_NAME_CHARS);
}

#[test]
fn peer_names_are_sanitized_on_receipt() {
    let (a, b) = (peer(), peer());
    let t = ticket(&a);
    let evil = format!("bob\u{202E}\u{7}{}", "x".repeat(100));
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, &evil, T0 + 1, GTTL).unwrap();
    let (_, a_view) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, T0 + 2, &none(), GTTL).unwrap();
    assert_eq!(a_view.name.chars().count(), MAX_NAME_CHARS);
    assert!(a_view.name.starts_with("bobxxx"));
    assert!(!a_view.name.contains('\u{202E}') && !a_view.name.contains('\u{7}'));
    // A ticket whose signed name carries control characters decodes clean.
    let t = issue_contact_ticket(&a.id, a.dev, "al\u{202E}ice\n", None, T0, TTL);
    assert_eq!(t.name, "alice");
    let back = ContactTicket::from_text(&t.to_text()).unwrap();
    assert_eq!(back.verify(T0 + 1).unwrap().name, "alice");
    assert_eq!(pending.name, "alice");
}

#[test]
fn old_format_messages_still_parse() {
    let a = peer();
    let g = issue_grant(&a.id, a.id.did(), T0, GTTL);
    let att = attest(&a.id, a.dev, T0);
    let hello = serde_json::json!({"t":"call_hello","call_id":"c","attestation":att,"grant":g});
    match serde_json::from_value::<Msg>(hello).unwrap() {
        Msg::CallHello { devices, relay, .. } => assert!(devices.is_none() && relay.is_none()),
        _ => panic!(),
    }
    let acc = serde_json::json!({"t":"accept","renewed_grant":null});
    assert_eq!(
        serde_json::from_value::<Msg>(acc).unwrap(),
        Msg::Accept {
            renewed_grant: None,
            devices: None,
            video: None
        }
    );
    // And without the new fields set, we emit exactly the old wire shape.
    let m = call_hello(&a.id, att, g, "c".into());
    assert!(!serde_json::to_string(&m).unwrap().contains("devices"));
    assert!(!serde_json::to_string(&m).unwrap().contains("video"));
}

#[test]
fn cancel_tokens() {
    for (r, t) in [
        (CancelReason::AnsweredElsewhere, "answered_elsewhere"),
        (CancelReason::DeclinedElsewhere, "declined_elsewhere"),
        (CancelReason::CallerHangup, "caller_hangup"),
    ] {
        let j = serde_json::to_value(Msg::Cancel { reason: r }).unwrap();
        assert_eq!(j, serde_json::json!({"t":"cancel","reason":t}));
    }
    assert!(serde_json::from_str::<Msg>(r#"{"t":"cancel","reason":"nope"}"#).is_err());
}

#[test]
fn call_hello_with_max_device_list_fits_frame() {
    let a = peer();
    let long = format!("https://{}.example/", "r".repeat(150));
    let devs: Vec<_> = (0..MAX_LIST_DEVICES)
        .map(|_| (dev(), Some(long.clone())))
        .collect();
    let list = sign_device_list(&a.id, &devs, u64::MAX);
    let mut m = call_hello(
        &a.id,
        attest(&a.id, a.dev, T0),
        issue_grant(&a.id, a.id.did(), T0, GTTL),
        "x".repeat(64),
    );
    if let Msg::CallHello { devices, relay, .. } = &mut m {
        *devices = Some(list);
        *relay = Some(long);
    }
    let f = encode_frame(&m);
    assert!(f.len() - 4 < MAX_FRAME, "{}", f.len());
    assert_eq!(decode_frame(&f).unwrap().unwrap().0, m);
}
