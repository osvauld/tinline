use super::*;

/// An envelope signed without the sign-time shape check: what a hostile peer can send.
fn raw<T: Signable>(v: &T, id: &Identity) -> Signed<T> {
    let body = cryptography::codec::to_bytes(v).unwrap();
    let sig = id.sign(&cryptography::sign::input(T::DOMAIN, &[&body]));
    Signed::from_parts(body, sig)
}

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
            let (j, b) = sign_join(&id, &gid(), cid, 1, 1_800_000_000_000 + k as u64).unwrap();
            RosterEntry::new(k as u8, &j, &b)
        })
        .collect();
    Roster {
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
    let s = Signed::sign(&start(&h, &cid), &h).unwrap();
    let GroupCallMsg::Start { start: s2 } = frame(&GroupCallMsg::Start { start: s.clone() }) else {
        panic!()
    };
    assert_eq!(s2.open().unwrap(), start(&h, &cid));
    let (j, jb) = sign_join(&m, &gid(), &cid, 1, 5).unwrap();
    assert_eq!(jb.open().unwrap(), j);
    let r = roster(&h, &cid, 3);
    assert_eq!(Signed::sign(&r, &h).unwrap().open().unwrap(), r);
    let l = sign_leave(&m, &gid(), &cid, 1, 6).unwrap().open().unwrap();
    assert_eq!((l.did.as_str(), l.issued_ms), (m.did(), 6));
    let e = End {
        gid: gid(),
        call_id: cid.clone(),
        host_gen: 1,
        host_did: h.did().into(),
        reason: EndReason::HostLeaving,
        next_host: Some(host_ref(&ident(90))),
    };
    assert_eq!(Signed::sign(&e, &h).unwrap().open().unwrap(), e);
    for msg in [
        GroupCallMsg::Join {
            join: jb.clone(),
            note: None,
        },
        GroupCallMsg::Join {
            join: jb,
            note: Some(sign_note(&m, &[1; 32], &[2; 32], 5)),
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
    // Signed by someone other than the claimed host: refused at sign time, and a raw envelope
    // carrying the stranger's signature does not open.
    assert_eq!(
        Signed::sign(&start(&h, &cid), &m).err(),
        Some(Error::WrongIssuer)
    );
    assert_eq!(
        raw(&start(&h, &cid), &m).open().err(),
        Some(Error::BadSignature)
    );
    // Tampered body.
    let mut s = start(&h, &cid);
    let good = Signed::sign(&s, &h).unwrap();
    s.k = 9;
    let tampered =
        Signed::<CallStart>::from_parts(cryptography::codec::to_bytes(&s).unwrap(), *good.sig());
    assert_eq!(tampered.open(), Err(Error::BadSignature));
    // A Join is not a Leave even though the fields overlap (own domains); a 1:1 grant is not a Join.
    let (_, jb) = sign_join(&m, &gid(), &cid, 1, 5).unwrap();
    // The same Leave body signed under Join's domain is not a Leave.
    let leave = Leave {
        gid: gid(),
        call_id: cid.clone(),
        host_gen: 1,
        did: m.did().into(),
        issued_ms: 5,
    };
    let body = cryptography::codec::to_bytes(&leave).unwrap();
    let sig = m.sign(&cryptography::sign::input(Join::DOMAIN, &[&body]));
    assert_eq!(
        Signed::<Leave>::from_parts(body, sig).open(),
        Err(Error::BadSignature)
    );
    // And a Join's bytes read as a Leave do not decode (different fields, own domain).
    assert!(retype::<Join, Leave>(&jb).open().is_err());
    let grant = issue_grant(&m, h.did(), 1, 100);
    assert!(retype::<Grant, Join>(&grant).open().is_err());
    // Shape bounds, at sign time and in a raw envelope.
    let mut bad = start(&h, &cid);
    bad.backups.push(host_ref(&ident(92)));
    assert_eq!(Signed::sign(&bad, &h).err(), Some(Error::Decode));
    assert_eq!(raw(&bad, &h).open().err(), Some(Error::Decode));
    let mut bad = start(&h, &cid);
    bad.k = 0;
    assert_eq!(raw(&bad, &h).open().err(), Some(Error::Decode));
    // Q2: K may name every participant (each hears the other eleven), never more.
    let mut all = start(&h, &cid);
    all.k = MAX_PARTICIPANTS as u8;
    assert_eq!(Signed::sign(&all, &h).unwrap().open(), Ok(all));
    let mut bad = start(&h, &cid);
    bad.k = MAX_PARTICIPANTS as u8 + 1;
    assert_eq!(raw(&bad, &h).open().err(), Some(Error::Decode));
    let mut bad = start(&h, &cid);
    bad.call_id = "short".into();
    assert_eq!(raw(&bad, &h).open().err(), Some(Error::Decode));
    // Size bound.
    let mut bad = start(&h, &cid);
    bad.meta_frontier = "m".repeat(MAX_CONTROL);
    assert_eq!(raw(&bad, &h).open().err(), Some(Error::TooLarge));
}

fn retype<T: Signable, U: Signable>(s: &Signed<T>) -> Signed<U> {
    Signed::from_bytes(&s.to_bytes()).unwrap()
}

fn request(starter: &Identity, host: &Identity, cid: &str) -> HostRequest {
    HostRequest {
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
    let rb = Signed::sign(&r, &a).unwrap();
    assert_eq!(rb.open().unwrap(), r);
    // Not by the starter; not another kind (each type has its own domain).
    assert_eq!(Signed::sign(&r, &h).err(), Some(Error::WrongIssuer));
    assert_eq!(raw(&r, &h).open().err(), Some(Error::BadSignature));
    assert!(retype::<HostRequest, CallStart>(&rb).open().is_err());
    assert!(retype::<HostRequest, HostRefusal>(&rb).open().is_err());
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
        assert_eq!(Signed::sign(&bad, &a).err(), Some(Error::Decode));
        assert_eq!(raw(&bad, &a).open().err(), Some(Error::Decode));
    }
    let f = HostRefusal {
        gid: gid(),
        call_id: cid.clone(),
        starter_did: a.did().into(),
        host: host_ref(&h),
        reason: "busy".into(),
    };
    let fb = Signed::sign(&f, &h).unwrap();
    assert_eq!(fb.open().unwrap(), f);
    assert_eq!(Signed::sign(&f, &a).err(), Some(Error::WrongIssuer));
    assert_eq!(raw(&f, &a).open().err(), Some(Error::BadSignature));
    let long = HostRefusal {
        reason: "r".repeat(MAX_TEXT + 1),
        ..f
    };
    assert_eq!(Signed::sign(&long, &h).err(), Some(Error::Decode));
    assert_eq!(raw(&long, &h).open().err(), Some(Error::Decode));
    let nb = sign_note(&a, &[7; 32], &[9; 32], 5);
    let n = nb.open().unwrap();
    assert_eq!(
        (n.did.as_str(), n.dev, n.endpoint),
        (a.did(), [7; 32], [9; 32])
    );
    assert!(retype::<GroupNote, Join>(&nb).open().is_err());
}

#[test]
fn gcall_roster_entries_are_bound_to_the_rosters_call_and_generation() {
    let h = ident(1);
    let cid = new_call_id();
    // An entry whose Join was for another call (or generation) fails, at sign and at open.
    let mut r = roster(&h, &cid, 2);
    let (j, b) = sign_join(&ident(60), &gid(), &new_call_id(), 1, 1).unwrap();
    r.entries.push(RosterEntry::new(5, &j, &b));
    assert_eq!(Signed::sign(&r, &h).err(), Some(Error::BadSignature));
    assert_eq!(raw(&r, &h).open().err(), Some(Error::BadSignature));
    let mut r = roster(&h, &cid, 2);
    r.host_gen = 2;
    assert_eq!(raw(&r, &h).open().err(), Some(Error::BadSignature));
    // A signature that is not 64 bytes.
    let mut r = roster(&h, &cid, 2);
    r.entries[0].sig.pop();
    assert_eq!(raw(&r, &h).open().err(), Some(Error::Decode));
    // Duplicate slot, duplicate DID, too many entries.
    let mut r = roster(&h, &cid, 2);
    r.entries[1].slot = r.entries[0].slot;
    assert_eq!(raw(&r, &h).open().err(), Some(Error::Decode));
    let mut r = roster(&h, &cid, 2);
    let mut dup = r.entries[0].clone();
    dup.slot = 9;
    r.entries.push(dup);
    assert_eq!(raw(&r, &h).open().err(), Some(Error::Decode));
    let r = roster(&h, &cid, MAX_PARTICIPANTS + 1);
    assert!(matches!(
        raw(&r, &h).open().err(),
        Some(Error::Decode | Error::TooLarge)
    ));
}

// §9 finding 1: a 12-entry Roster (64-char gid, max epoch and slot values) fits.
#[test]
fn gcall_roster_of_12_fits_the_control_limit() {
    let h = ident(1);
    let cid = new_call_id();
    let blob = Signed::sign(&roster(&h, &cid, MAX_PARTICIPANTS), &h).unwrap();
    let bytes = encode_control(&GroupCallMsg::Roster {
        roster: blob.clone(),
    })
    .unwrap();
    assert!(bytes.len() - 4 <= MAX_CONTROL);
    let (msg, _) = decode_control(&bytes).unwrap().unwrap();
    let GroupCallMsg::Roster { roster: back } = msg else {
        panic!()
    };
    assert_eq!(back.open().unwrap().entries.len(), MAX_PARTICIPANTS);
    // A CallStart with two backups is far below the limit as well.
    let s = encode_control(&GroupCallMsg::Start {
        start: Signed::sign(&start(&h, &cid), &h).unwrap(),
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
