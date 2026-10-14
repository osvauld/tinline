use super::*;

fn ident(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

fn gid() -> String {
    "f0".repeat(32) // 64 chars: a hex group id, the long case
}

fn host_ref(id: &Identity) -> HostRef {
    HostRef {
        holder_did: id.did().into(),
        device_slot: u64::MAX,
        permit_digest: "d".repeat(64),
    }
}

fn start(id: &Identity, cid: &str) -> CallStart {
    CallStart {
        v: GCALL_VERSION,
        gid: gid(),
        call_id: cid.into(),
        host_gen: 1,
        host_ref: host_ref(id),
        starter_did: id.did().into(),
        started_ms: 1_800_000_000_000,
        k: 3,
        max_participants: 12,
        backups: vec![host_ref(&ident(90)), host_ref(&ident(91))],
        meta_frontier: "m".repeat(64),
    }
}

fn roster(host: &Identity, cid: &str, n: usize) -> Roster {
    let entries = (0..n)
        .map(|k| {
            let id = ident(40 + k as u8);
            let (j, b) = sign_join(&id, &gid(), cid, 1, 1_800_000_000_000 + k as u64);
            RosterEntry::new(k as u8, &j, &b)
        })
        .collect();
    Roster {
        v: GCALL_VERSION,
        gid: gid(),
        call_id: cid.into(),
        host_gen: 1,
        host_did: host.did().into(),
        epoch: u32::MAX,
        entries,
    }
}

fn frame(m: &GroupCallMsg) -> GroupCallMsg {
    let bytes = encode_control(m).unwrap();
    let (back, used) = decode_control(&bytes).unwrap().unwrap();
    assert_eq!(used, bytes.len());
    back
}

#[test]
fn gcall_signed_messages_round_trip_through_frames() {
    let h = ident(1);
    let m = ident(2);
    let cid = new_call_id();
    let s = sign_start(&h, start(&h, &cid));
    let GroupCallMsg::Start { start: s2 } = frame(&GroupCallMsg::Start { start: s.clone() }) else {
        panic!()
    };
    assert_eq!(open_start(&s2).unwrap(), start(&h, &cid));
    let (j, jb) = sign_join(&m, &gid(), &cid, 1, 5);
    assert_eq!(open_join(&jb).unwrap(), j);
    let r = roster(&h, &cid, 3);
    assert_eq!(open_roster(&sign_roster(&h, r.clone())).unwrap(), r);
    let l = open_leave(&sign_leave(&m, &gid(), &cid, 1, 6)).unwrap();
    assert_eq!((l.did.as_str(), l.issued_ms), (m.did(), 6));
    let e = End {
        v: GCALL_VERSION,
        gid: gid(),
        call_id: cid.clone(),
        host_gen: 1,
        host_did: h.did().into(),
        reason: EndReason::HostLeaving,
        next_host: Some(host_ref(&ident(90))),
    };
    assert_eq!(open_end(&sign_end(&h, e.clone())).unwrap(), e);
    for msg in [
        GroupCallMsg::Join {
            join: jb.clone(),
            note: None,
        },
        GroupCallMsg::Join {
            join: jb,
            note: Some(s),
        },
        GroupCallMsg::Heartbeat { epoch: 7 },
        GroupCallMsg::Active {
            r#gen: 3,
            slots: vec![0, 4],
        },
    ] {
        assert_eq!(frame(&msg), msg);
    }
}

#[test]
fn gcall_forged_tampered_and_cross_kind_blobs_are_refused() {
    let h = ident(1);
    let m = ident(2);
    let cid = new_call_id();
    // Signed by someone other than the claimed host.
    let forged = sign_blob(&m, GCALL_DOMAIN, &Claim::Start(start(&h, &cid)));
    assert_eq!(open_start(&forged), Err(Error::BadSignature));
    // Tampered payload.
    let mut s = start(&h, &cid);
    let good = sign_start(&h, s.clone());
    s.k = 9;
    let tampered = SignedBlob {
        payload: enc(serde_json::to_vec(&Claim::Start(s)).unwrap()),
        signature: good.signature,
    };
    assert_eq!(open_start(&tampered), Err(Error::BadSignature));
    // A Join is not a Leave even though the fields overlap; a 1:1 grant is not a Join.
    let (_, jb) = sign_join(&m, &gid(), &cid, 1, 5);
    assert_eq!(open_leave(&jb), Err(Error::UnexpectedMessage));
    let other_domain = sign_blob(
        &m,
        b"osvauld/p2p/grant/v1\0",
        &Claim::Join(open_join(&jb).unwrap()),
    );
    assert_eq!(open_join(&other_domain), Err(Error::BadSignature));
    // Shape bounds.
    let mut bad = start(&h, &cid);
    bad.backups.push(host_ref(&ident(92)));
    assert_eq!(open_start(&sign_start(&h, bad)), Err(Error::Decode));
    let mut bad = start(&h, &cid);
    bad.k = 0;
    assert_eq!(open_start(&sign_start(&h, bad)), Err(Error::Decode));
    let mut bad = start(&h, &cid);
    bad.call_id = "short".into();
    assert_eq!(open_start(&sign_start(&h, bad)), Err(Error::Decode));
    let mut bad = start(&h, &cid);
    bad.v = 2;
    assert_eq!(open_start(&sign_start(&h, bad)), Err(Error::UnknownVersion));
}

fn request(starter: &Identity, host: &Identity, cid: &str) -> HostRequest {
    HostRequest {
        v: GCALL_VERSION,
        gid: gid(),
        call_id: cid.into(),
        starter_did: starter.did().into(),
        host: host_ref(host),
        backups: vec![host_ref(&ident(90)), host_ref(&ident(91))],
        expected: 12,
        issued_ms: 1_800_000_000_000,
    }
}

// GA-I1: the starter's CallHostRequest, the candidate's refusal and the group endpoint note
// round-trip, are bound to their signer and kind, and are bounded.
#[test]
fn gcall_host_request_refusal_and_note_verify_and_are_bounded() {
    let a = ident(1);
    let h = ident(2);
    let cid = new_call_id();
    let r = request(&a, &h, &cid);
    let rb = sign_host_request(&a, r.clone());
    assert_eq!(open_host_request(&rb).unwrap(), r);
    // Not by the starter; not another kind.
    assert_eq!(
        open_host_request(&sign_blob(&h, GCALL_DOMAIN, &Claim::HostRequest(r.clone()))),
        Err(Error::BadSignature)
    );
    assert_eq!(open_start(&rb), Err(Error::UnexpectedMessage));
    assert_eq!(open_host_refusal(&rb), Err(Error::UnexpectedMessage));
    for bad in [
        HostRequest {
            backups: vec![host_ref(&ident(90)); 3],
            ..r.clone()
        },
        HostRequest {
            expected: 1,
            ..r.clone()
        },
        HostRequest {
            expected: 13,
            ..r.clone()
        },
        HostRequest {
            call_id: "x".into(),
            ..r.clone()
        },
        HostRequest {
            host: HostRef {
                permit_digest: String::new(),
                ..host_ref(&h)
            },
            ..r.clone()
        },
    ] {
        assert_eq!(
            open_host_request(&sign_host_request(&a, bad)),
            Err(Error::Decode)
        );
    }
    let f = HostRefusal {
        v: GCALL_VERSION,
        gid: gid(),
        call_id: cid.clone(),
        starter_did: a.did().into(),
        host: host_ref(&h),
        reason: "busy".into(),
    };
    let fb = sign_host_refusal(&h, f.clone());
    assert_eq!(open_host_refusal(&fb).unwrap(), f);
    assert_eq!(
        open_host_refusal(&sign_blob(&a, GCALL_DOMAIN, &Claim::HostRefusal(f.clone()))),
        Err(Error::BadSignature)
    );
    let long = HostRefusal {
        reason: "r".repeat(MAX_TEXT + 1),
        ..f
    };
    assert_eq!(
        open_host_refusal(&sign_host_refusal(&h, long)),
        Err(Error::Decode)
    );
    let nb = sign_note(&a, &[7; 32], &[9; 32], 5);
    let n = open_note(&nb).unwrap();
    assert_eq!(
        (n.did.as_str(), n.dev_bytes(), n.endpoint_bytes()),
        (a.did(), Some([7; 32]), Some([9; 32]))
    );
    assert_eq!(open_join(&nb), Err(Error::UnexpectedMessage));
    let short = GroupNote {
        endpoint: enc([1u8; 31]),
        ..n
    };
    assert_eq!(
        open_note(&sign_blob(&a, GCALL_DOMAIN, &Claim::Note(short))),
        Err(Error::Decode)
    );
}

#[test]
fn gcall_roster_entries_are_bound_to_the_rosters_call_and_generation() {
    let h = ident(1);
    let cid = new_call_id();
    // An entry whose Join was for another call (or generation) fails.
    let mut r = roster(&h, &cid, 2);
    let (j, b) = sign_join(&ident(60), &gid(), &new_call_id(), 1, 1);
    r.entries.push(RosterEntry::new(5, &j, &b));
    assert_eq!(open_roster(&sign_roster(&h, r)), Err(Error::BadSignature));
    let mut r = roster(&h, &cid, 2);
    r.host_gen = 2;
    assert_eq!(open_roster(&sign_roster(&h, r)), Err(Error::BadSignature));
    // Duplicate slot, duplicate DID, too many entries.
    let mut r = roster(&h, &cid, 2);
    r.entries[1].slot = r.entries[0].slot;
    assert_eq!(open_roster(&sign_roster(&h, r)), Err(Error::Decode));
    let mut r = roster(&h, &cid, 2);
    let mut dup = r.entries[0].clone();
    dup.slot = 9;
    r.entries.push(dup);
    assert_eq!(open_roster(&sign_roster(&h, r)), Err(Error::Decode));
    let r = roster(&h, &cid, MAX_PARTICIPANTS + 1);
    assert_eq!(open_roster(&sign_roster(&h, r)), Err(Error::Decode));
}

// §9 finding 1: a 12-entry Roster (64-char gid, max epoch and slot values) fits.
#[test]
fn gcall_roster_of_12_fits_the_control_limit() {
    let h = ident(1);
    let cid = new_call_id();
    let blob = sign_roster(&h, roster(&h, &cid, MAX_PARTICIPANTS));
    let bytes = encode_control(&GroupCallMsg::Roster {
        roster: blob.clone(),
    })
    .unwrap();
    println!(
        "12-entry Roster frame: {} B of {MAX_CONTROL}",
        bytes.len() - 4
    );
    assert!(bytes.len() - 4 <= MAX_CONTROL);
    let (msg, _) = decode_control(&bytes).unwrap().unwrap();
    let GroupCallMsg::Roster { roster: back } = msg else {
        panic!()
    };
    assert_eq!(open_roster(&back).unwrap().entries.len(), MAX_PARTICIPANTS);
    // A CallStart with two backups is far below the limit as well.
    let s = encode_control(&GroupCallMsg::Start {
        start: sign_start(&h, start(&h, &cid)),
    })
    .unwrap();
    assert!(s.len() < MAX_CONTROL / 2, "{}", s.len());
}

#[test]
fn gcall_control_frames_are_bounded_before_buffering() {
    let mut huge = ((MAX_CONTROL + 1) as u32).to_be_bytes().to_vec();
    huge.extend([b' '; 8]);
    assert_eq!(decode_control(&huge), Err(Error::FrameTooLarge));
    let ok = encode_control(&GroupCallMsg::Heartbeat { epoch: 1 }).unwrap();
    assert_eq!(decode_control(&ok[..ok.len() - 1]), Ok(None));
    assert_eq!(decode_control(&ok[..3]), Ok(None));
    let many = encode_json_frame(&GroupCallMsg::Active {
        r#gen: 1,
        slots: vec![0; MAX_PARTICIPANTS + 1],
    });
    assert_eq!(decode_control(&many), Err(Error::Decode));
    let mut garbage = 3u32.to_be_bytes().to_vec();
    garbage.extend(b"{x}");
    assert_eq!(decode_control(&garbage), Err(Error::Decode));
    let big = GroupCallMsg::Active {
        r#gen: 1,
        slots: vec![200; 2000],
    };
    assert_eq!(encode_control(&big), Err(Error::FrameTooLarge));
}

#[test]
fn gcall_datagram_codecs_parse_bounded_input() {
    let audio: Vec<u8> = [&7u32.to_be_bytes()[..], &[20], &[0xA5; 120]].concat();
    let up = up_audio(&audio);
    assert_eq!(parse_up(&up), Ok(UpDatagram::Audio(&audio[..])));
    assert_eq!(audio_level(&audio), 20);
    assert_eq!(parse_up(&up_level(70)), Ok(UpDatagram::Level(70)));
    let down = down_audio(4, &audio);
    assert_eq!(parse_down(&down), Ok((4, &audio[..])));
    // Largest legal upstream audio forwards into a legal downstream datagram.
    let max = up_audio(&vec![1u8; MAX_UP_DATAGRAM - 1]);
    let Ok(UpDatagram::Audio(a)) = parse_up(&max) else {
        panic!()
    };
    assert!(parse_down(&down_audio(0, a)).is_ok());
    for bad in [
        vec![],
        vec![UP_AUDIO],
        vec![UP_AUDIO, 0, 0, 0, 1, 20], // header only, no Opus
        vec![UP_LEVEL],
        vec![UP_LEVEL, 1, 2],
        vec![DOWN_AUDIO, 0, 0, 0, 0, 1, 20, 1],
        vec![0x09, 1, 2, 3, 4, 5, 6],
    ] {
        assert_eq!(parse_up(&bad), Err(Error::Decode), "{bad:?}");
    }
    assert_eq!(
        parse_up(&up_audio(&vec![1u8; MAX_UP_DATAGRAM])),
        Err(Error::FrameTooLarge)
    );
    for bad in [
        vec![],
        vec![DOWN_AUDIO],
        vec![DOWN_AUDIO, 1, 0, 0, 0, 1, 20],
        up.clone(),
    ] {
        assert_eq!(parse_down(&bad), Err(Error::Decode), "{bad:?}");
    }
}
