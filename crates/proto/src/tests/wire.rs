//! Wire frames, name sanitising and message compatibility.

use super::*;

#[test]
fn frame_roundtrip_all_variants() {
    let a = peer();
    let g = issue_grant(&a.id, a.id.did(), M0, GTTL);
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
                max_fps: 30,
                decode: vec![VideoCodec::Av1, VideoCodec::Vp8],
                encode: vec![VideoCodec::Vp8, VideoCodec::Av1],
                hw_decode: vec![VideoCodec::Vp8],
                hw_encode: vec![VideoCodec::Vp8],
                hint_flags: true,
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
    let (hello, pending) = contact_hello(&b.id, b.dev, &t, &evil, M0 + 1, GTTL).unwrap();
    let (_, a_view) =
        accept_contact_hello(&a.id, a.dev, &hello, b.dev, M0 + 2, &Unrecorded, GTTL).unwrap();
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
    let g = issue_grant(&a.id, a.id.did(), M0, GTTL);
    let att = attest(&a.id, a.dev, T0);
    let hello = serde_json::json!({"t":"call_hello","call_id":"c","attestation":att,"grant":g});
    match serde_json::from_value::<Msg>(hello).unwrap() {
        Msg::CallHello { devices, relay, camera, .. } => assert!(devices.is_none() && relay.is_none() && !camera),
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
    let m = call_hello(att, g, "c".into());
    assert!(!serde_json::to_string(&m).unwrap().contains("devices"));
    assert!(!serde_json::to_string(&m).unwrap().contains("video"));
    assert!(!serde_json::to_string(&m).unwrap().contains("camera"));
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
        attest(&a.id, a.dev, T0),
        issue_grant(&a.id, a.id.did(), M0, GTTL),
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

/// Every signed statement runs the same envelope table (forgery, cross-domain replay, trailing
/// bytes, wrong signer, size cap). `Twin` and the mutations live in `signed::conformance`.
#[test]
fn every_signable_passes_the_envelope_table() {
    use crate::gcall::{self, *};
    use crate::gcall_status::*;
    use signed::conformance::check;
    let (a, b) = (peer(), peer());
    let (x, y) = (&a.id, &b.id);
    let cid = new_call_id();
    let gid = "f0".repeat(32);
    let hr = |id: &Identity| HostRef {
        holder_did: id.did().into(),
        device_slot: 3,
        permit_digest: "d".repeat(64),
    };

    check(&attest(x, a.dev, T0).peek().unwrap(), x, y);
    check(&issue_grant(x, y.did(), M0, GTTL).peek().unwrap(), x, y);
    // A ticket is signed by the raw account key it carries.
    check(&ticket(&a).invite.peek().unwrap(), x, y);
    check(
        &sign_device_list(
            x,
            &[(a.dev, None), (dev(), Some("https://r.example/".into()))],
            7,
        )
        .peek()
        .unwrap(),
        x,
        y,
    );
    check(
        &sign_block_notice(x, y.did(), true, 9).peek().unwrap(),
        x,
        y,
    );

    let (join, join_env) = gcall::sign_join(y, &gid, &cid, 1, 5).unwrap();
    check(&join, y, x);
    check(
        &sign_leave(y, &gid, &cid, 1, 6).unwrap().peek().unwrap(),
        y,
        x,
    );
    check(
        &Roster {
            gid: gid.clone(),
            call_id: cid.clone(),
            host_gen: 1,
            host_did: x.did().into(),
            epoch: 2,
            entries: vec![RosterEntry::new(1, &join, &join_env)],
        },
        x,
        y,
    );
    check(
        &CallStart {
            gid: gid.clone(),
            call_id: cid.clone(),
            host_gen: 1,
            host_ref: hr(x),
            starter_did: x.did().into(),
            started_ms: T0,
            k: 3,
            max_participants: 12,
            backups: vec![hr(y)],
            meta_frontier: "m".into(),
        },
        x,
        y,
    );
    check(
        &End {
            gid: gid.clone(),
            call_id: cid.clone(),
            host_gen: 1,
            host_did: x.did().into(),
            reason: EndReason::Ended,
            next_host: Some(hr(y)),
        },
        x,
        y,
    );
    check(
        &HostRequest {
            gid: gid.clone(),
            call_id: cid.clone(),
            starter_did: x.did().into(),
            host: hr(y),
            backups: vec![],
            expected: 4,
            issued_ms: T0,
        },
        x,
        y,
    );
    check(
        &HostRefusal {
            gid: gid.clone(),
            call_id: cid,
            starter_did: x.did().into(),
            host: hr(y),
            reason: "busy".into(),
        },
        y,
        x,
    );
    check(&sign_note(x, &a.dev, &[9; 32], 5).peek().unwrap(), x, y);
    check(
        &CallHostStatus {
            gid,
            host_ref: hr(x),
            class: DeviceClass::Phone,
            power: PowerState::Battery(50),
            network: NetworkKind::Metered,
            thermal: ThermalLevel::None,
            busy: false,
            load: 10,
            max_participants: 6,
            issued_ms: T0,
            ttl_ms: 30_000,
        },
        x,
        y,
    );
}

fn offer(decode: &[VideoCodec], encode: &[VideoCodec], hw_decode: &[VideoCodec], hw_encode: &[VideoCodec]) -> VideoOffer {
    VideoOffer {
        max_pixels: 1280 * 720,
        max_fps: 30,
        decode: decode.to_vec(),
        encode: encode.to_vec(),
        hw_decode: hw_decode.to_vec(),
        hw_encode: hw_encode.to_vec(),
        hint_flags: false,
    }
}

/// `VideoOffer.hint_flags` is optional both ways: an offer without it (an older device) reads as
/// false, and false is not written (the offer's JSON stays what older devices send).
#[test]
fn video_offer_hint_flags_optional() {
    let o = offer(&[VideoCodec::Av1], &[VideoCodec::Av1], &[], &[]);
    let json = serde_json::to_string(&o).unwrap();
    assert!(!json.contains("hint_flags"), "{json}");
    assert_eq!(serde_json::from_str::<VideoOffer>(&json).unwrap(), o);
    let on = VideoOffer { hint_flags: true, ..o };
    let json = serde_json::to_string(&on).unwrap();
    assert!(json.contains("\"hint_flags\":true"), "{json}");
    assert_eq!(serde_json::from_str::<VideoOffer>(&json).unwrap(), on);
}

#[test]
fn send_codec_table() {
    use VideoCodec::{Av1, Vp8};
    // Pixel-class phone: VP8 in hardware both ways. Desktop: libaom and libvpx in software.
    // Soft phone: a software VP8 decoder only. AV1-only: nothing else.
    let pixel = offer(&[Av1, Vp8], &[Vp8, Av1], &[Vp8], &[Vp8]);
    let desktop = offer(&[Av1, Vp8], &[Av1, Vp8], &[], &[]);
    let soft_phone = offer(&[Av1, Vp8], &[Av1], &[], &[]);
    let hw_dec_only = offer(&[Av1, Vp8], &[Av1], &[Vp8], &[]);
    let av1_only = offer(&[Av1], &[Av1], &[], &[]);
    let cases = [
        (&desktop, &desktop, Av1),
        (&pixel, &desktop, Vp8),
        (&desktop, &pixel, Vp8),
        (&pixel, &pixel, Vp8),
        (&desktop, &soft_phone, Av1),
        (&soft_phone, &desktop, Av1),
        (&pixel, &soft_phone, Vp8), // the sender's hardware encoder alone is enough
        (&soft_phone, &pixel, Av1), // soft phone only encodes AV1: no VP8 candidate
        (&desktop, &hw_dec_only, Vp8),
        (&pixel, &av1_only, Av1),
        (&av1_only, &pixel, Av1),
        (&av1_only, &av1_only, Av1),
    ];
    for (i, (s, r, want)) in cases.into_iter().enumerate() {
        assert_eq!(send_codec(s, r), want, "case {i}");
        assert_eq!(video_codecs(s, r).map(|(tx, _)| tx), Some(want), "validated case {i}");
    }
    // Ties go to the sender's encode order; an empty or odd offer falls back to AV1.
    assert_eq!(send_codec(&offer(&[Av1, Vp8], &[Av1, Vp8], &[], &[]), &offer(&[Vp8, Av1], &[], &[], &[])), Av1);
    assert_eq!(send_codec(&offer(&[Av1, Vp8], &[Vp8, Av1], &[], &[]), &offer(&[Av1, Vp8], &[], &[], &[])), Vp8);
    assert_eq!(send_codec(&offer(&[], &[], &[], &[]), &offer(&[], &[], &[], &[])), Av1);
    assert_eq!(send_codec(&offer(&[Av1], &[Vp8], &[], &[Vp8]), &offer(&[Av1], &[], &[], &[])), Av1);
}

#[test]
fn filtered_video_offers_require_a_common_codec_in_both_directions() {
    use VideoCodec::{Av1, Vp8};
    let pixel = offer(&[Av1, Vp8], &[Vp8, Av1], &[Vp8], &[Vp8]);
    let av1_forced = offer(&[Av1, Vp8], &[Av1], &[Vp8], &[]);
    assert_eq!(video_codecs(&av1_forced, &pixel), Some((Av1, Vp8)));
    assert_eq!(video_codecs(&pixel, &av1_forced), Some((Vp8, Av1)));
    let vp8_forced = offer(&[Av1, Vp8], &[Vp8], &[], &[]);
    let desktop = offer(&[Av1, Vp8], &[Av1, Vp8], &[], &[]);
    assert_eq!(video_codecs(&vp8_forced, &desktop), Some((Vp8, Av1)));
    let av1_only = offer(&[Av1], &[Av1], &[], &[]);
    assert_eq!(video_codecs(&vp8_forced, &av1_only), None);
    assert_eq!(video_codecs(&av1_only, &vp8_forced), None);
    assert_eq!(video_codecs(&offer(&[], &[], &[], &[]), &desktop), None);
}

#[test]
fn video_offer_json_shape() {
    let o = offer(&[VideoCodec::Av1, VideoCodec::Vp8], &[VideoCodec::Av1, VideoCodec::Vp8], &[VideoCodec::Vp8], &[]);
    let j = serde_json::to_string(&o).unwrap();
    assert_eq!(
        j,
        r#"{"max_pixels":921600,"max_fps":30,"decode":["av1","vp8"],"encode":["av1","vp8"],"hw_decode":["vp8"],"hw_encode":[]}"#
    );
    assert_eq!(serde_json::from_str::<VideoOffer>(&j).unwrap(), o);
    // Unknown codecs are not part of the pre-release wire; they fail to parse.
    assert!(
        serde_json::from_str::<VideoOffer>(r#"{"max_pixels":1,"max_fps":30,"decode":["h264"],"encode":[],"hw_decode":[],"hw_encode":[]}"#)
            .is_err()
    );
}
