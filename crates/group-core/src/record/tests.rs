use super::*;

fn acct() -> identity::Identity {
    identity::generate().0
}

/// An envelope over `x` signed by `key` whatever `x` names as its signer.
fn forge<T: Signable>(x: &T, key: &identity::Identity) -> Signed<T> {
    let body = cryptography::codec::to_bytes(x).unwrap();
    let sig = key.sign(&cryptography::sign::input(T::DOMAIN, &[&body]));
    Signed::from_parts(body, sig)
}

fn rec(a: &identity::Identity, group: &str, body: Body) -> Record {
    Record {
        group: group.into(),
        author: a.did().into(),
        dev: [7; 32],
        at: 1000,
        seen: vec![(1, 2)],
        body,
    }
}

// KIND: unit (G1 stage 1)
#[test]
fn records_verify_and_tampering_is_refused() {
    let a = acct();
    let r = rec(&a, "g", Body::Post { text: "hi".into(), reply_to: None, image: None });
    let bytes = r.sign(&a).to_bytes();
    assert_eq!(open_record(&bytes, "g").unwrap(), r);
    assert_eq!(open_record(&bytes, "other"), Err(RecordError::WrongGroup));
    // Any flipped byte: undecodable or a bad signature, never accepted.
    for i in 0..bytes.len() {
        let mut t = bytes.clone();
        t[i] ^= 0x41;
        assert!(open_record(&t, "g").is_err(), "byte {i}");
    }
    // Trailing junk: a second id for the same record is refused.
    let mut t = bytes.clone();
    t.push(0);
    assert!(open_record(&t, "g").is_err());
    // Claiming another author: the signature is the signer's, not the claimed DID's.
    let b = acct();
    let mut forged = rec(&a, "g", Body::Post { text: "as b".into(), reply_to: None, image: None });
    forged.author = b.did().into();
    let bytes = forge(&forged, &a).to_bytes();
    assert_eq!(open_record(&bytes, "g"), Err(RecordError::BadSignature));
}

// KIND: unit (G1 stage 1)
#[test]
fn malformed_records_are_refused() {
    let a = acct();
    let cases = [
        Body::Post { text: String::new(), reply_to: None, image: None },
        Body::Post { text: "x".repeat(MAX_TEXT + 1), reply_to: None, image: None },
        Body::Genesis { name: " ".into(), nonce: [0; 16], owner_name: "o".into() },
        Body::CallHostPermit { host: "d".repeat(201), host_dev: [0; 32], active: true },
    ];
    for b in cases {
        assert_eq!(
            open_record(&forge(&rec(&a, "g", b), &a).to_bytes(), "g"),
            Err(RecordError::Malformed)
        );
    }
    let mut r = rec(&a, "g", Body::Leave);
    r.seen = vec![(5, 1), (3, 1)];
    assert_eq!(
        open_record(&forge(&r, &a).to_bytes(), "g"),
        Err(RecordError::Malformed)
    );
}

// KIND: unit (G1 stage 1)
#[test]
fn invites_host_addrs_and_auth_verify() {
    let a = acct();
    let b = acct();
    let inv = issue_invite(&a, "g", 1, 10);
    assert_eq!(inv.open().unwrap().iss, a.did());
    let theirs = Invite {
        iss: b.did().into(),
        sub: proto::Subject::Anyone,
        iat_ms: 1,
        exp_ms: 10,
        nonce: [1; 16],
        claim: GroupJoin { group: "g".into() },
    };
    assert!(
        Signed::sign(&theirs, &a).is_err(),
        "cannot sign as another issuer"
    );
    assert!(
        forge(&theirs, &a).open().is_err(),
        "claimed issuer did not sign"
    );
    assert!(inv.open_from(b.did()).is_err());
    let mut raw = inv.to_bytes();
    *raw.last_mut().unwrap() ^= 1;
    assert!(
        Signed::<Invite>::from_bytes(&raw)
            .and_then(|s| s.open())
            .is_err()
    );

    let ha = Signed::sign(
        &HostAddr {
            group: "g".into(),
            did: a.did().into(),
            dev: [1; 32],
            endpoint: [2; 32],
            relay: None,
            at: 5,
        },
        &a,
    )
    .unwrap();
    assert!(ha.open().is_ok());
    let mut raw = ha.to_bytes();
    raw[3] ^= 1;
    assert!(
        Signed::<HostAddr>::from_bytes(&raw)
            .and_then(|s| s.open())
            .is_err()
    );

    let auth = endpoint_auth(&a, [1; 32], [9; 32]);
    assert!(verify_endpoint(&auth, &[9; 32]).is_some());
    assert!(
        verify_endpoint(&auth, &[8; 32]).is_none(),
        "made for another endpoint"
    );
    let note = auth.open().unwrap();
    let other = proto::gcall::GroupNote {
        did: b.did().into(),
        ..note
    };
    assert!(
        verify_endpoint(&forge(&other, &a), &[9; 32]).is_none(),
        "claimed DID did not sign"
    );

    let tok = InviteToken { group: "g".into(), name: "n".into(), owner: a.did().into(), invite: Some(inv), hosts: vec![ha], open: false };
    assert_eq!(cryptography::codec::from_bytes::<InviteToken>(&cryptography::codec::to_bytes(&tok).unwrap(), usize::MAX).unwrap(), tok);
}

#[test]
fn group_ids_bind_owner_and_nonce() {
    assert_ne!(group_id("did:a", &[0; 16]), group_id("did:b", &[0; 16]));
    assert_ne!(group_id("did:a", &[0; 16]), group_id("did:a", &[1; 16]));
}

// The shared envelope property table for every statement type this crate signs.
#[test]
fn core_signables_pass_the_envelope_conformance_table() {
    use crate::mail::Mail;
    use crate::{attestation::StorageAttestation, doc::DocId};
    use receipts_core::{Conversation, Kind, Receipt, Scope};
    use proto::signed::conformance::check;
    let (a, stranger) = (acct(), acct());
    check(
        &rec(
            &a,
            "g",
            Body::Post {
                text: "hi".into(),
                reply_to: None,
                image: None,
            },
        ),
        &a,
        &stranger,
    );
    check(
        &Invite {
            iss: a.did().into(),
            sub: proto::Subject::Anyone,
            iat_ms: 1,
            exp_ms: 10,
            nonce: [1; 16],
            claim: GroupJoin { group: "g".into() },
        },
        &a,
        &stranger,
    );
    check(
        &HostAddr {
            group: "g".into(),
            did: a.did().into(),
            dev: [1; 32],
            endpoint: [2; 32],
            relay: None,
            at: 5,
        },
        &a,
        &stranger,
    );
    check(
        &Presence {
            group: "g".into(),
            host: a.did().into(),
            dev: [1; 32],
            online: vec![(stranger.did().into(), 4)],
            seq: 1,
            at: 5,
        },
        &a,
        &stranger,
    );
    check(
        &Mail {
            group: "g".into(),
            from: a.did().into(),
            to: stranger.did().into(),
            id: [4; 16],
            at: 9,
            sealed: vec![1, 2, 3],
        },
        &a,
        &stranger,
    );
    check(
        &Receipt {
            call: None,
            scope: Scope {
                conversation: Conversation::Direct("a".repeat(64)),
                day: "1970-01-01".into(),
            },
            kind: Kind::Delivered,
            recipient: a.did().into(),
            device: [1; 32],
            messages: vec![[1; 32]],
            metadata: vec![],
        },
        &a,
        &stranger,
    );
    check(
        &StorageAttestation {
            group: "a".repeat(64),
            doc: DocId::Day("2026-10-14".into()),
            host: a.did().into(),
            host_dev: [7; 32],
            stored: vec![(1, 3)],
            metadata: vec![(2, 4)],
        },
        &a,
        &stranger,
    );
}

// KIND: unit (public group 7) — a presence frame verifies only as its host's; a forged signer,
// a tampered body and more than `MAX_PRESENCE` DIDs are refused.
#[test]
fn presence_frames_verify_and_are_bounded() {
    let (h, m) = (acct(), acct());
    let p = Presence { group: "g".into(), host: h.did().into(), dev: [1; 32], online: vec![(m.did().into(), 9)], seq: 3, at: 10 };
    let signed = Signed::sign(&p, &h).unwrap();
    assert_eq!(signed.open().unwrap(), p);
    assert!(Signed::sign(&p, &m).is_err(), "only the host signs as the host");
    assert!(forge(&p, &m).open().is_err(), "the named host did not sign");
    let mut raw = signed.to_bytes();
    raw[4] ^= 1;
    assert!(Signed::<Presence>::from_bytes(&raw).and_then(|s| s.open()).is_err());
    let big = Presence { online: (0..=MAX_PRESENCE).map(|i| (format!("did:key:z{i}"), 1)).collect(), ..p.clone() };
    assert!(Signed::sign(&big, &h).is_err());
    assert!(forge(&big, &h).open().is_err());
    let full = Presence { online: (0..MAX_PRESENCE).map(|i| (format!("did:key:z6Mk{i:0>44}"), u64::MAX)).collect(), ..p };
    assert!(Signed::sign(&full, &h).unwrap().open().is_ok(), "500 DIDs fit one frame");
}

fn image(size: u64, w: u32, h: u32, mime: &str) -> ImageRef {
    ImageRef { hash: [1; 32], key: [2; 32], size, w, h, mime: mime.into(), preview: vec![0; 36] }
}

// KIND: unit (public group §5) — a post may carry an image (and then no text); the image's
// declared size, sides, type and preview are bounded on every device.
#[test]
fn image_posts_are_bounded() {
    let a = acct();
    let post = |text: &str, img: ImageRef| Body::Post { text: text.into(), reply_to: None, image: Some(img) };
    for ok in [
        post("", image(300_000, 1080, 720, "image/jpeg")),
        post("caption", image(MAX_IMAGE_BYTES, MAX_IMAGE_SIDE, 1, "image/webp")),
        post("", image(1, 1, MAX_IMAGE_SIDE, "image/png")),
    ] {
        let r = rec(&a, "g", ok);
        assert_eq!(open_record(&r.sign(&a).to_bytes(), "g").unwrap(), r);
    }
    let mut no_preview = image(10, 10, 10, "image/jpeg");
    no_preview.preview.clear();
    let r = rec(&a, "g", post("", no_preview));
    assert!(open_record(&r.sign(&a).to_bytes(), "g").is_ok());
    let mut big_preview = image(10, 10, 10, "image/jpeg");
    big_preview.preview = vec![0; MAX_IMAGE_PREVIEW + 1];
    for bad in [
        post("", image(MAX_IMAGE_BYTES + 1, 10, 10, "image/jpeg")),
        post("", image(0, 10, 10, "image/jpeg")),
        post("", image(10, MAX_IMAGE_SIDE + 1, 10, "image/jpeg")),
        post("", image(10, 10, MAX_IMAGE_SIDE + 1, "image/jpeg")),
        post("", image(10, 0, 10, "image/jpeg")),
        post("", image(10, 10, 10, "image/gif")),
        post("", image(10, 10, 10, "text/html")),
        post("", big_preview),
        post(&"x".repeat(MAX_TEXT + 1), image(10, 10, 10, "image/jpeg")),
    ] {
        assert_eq!(open_record(&forge(&rec(&a, "g", bad), &a).to_bytes(), "g"), Err(RecordError::Malformed));
    }
}

fn voice(size: u64, duration_ms: u32, bars: usize) -> VoiceRef {
    VoiceRef { hash: [1; 32], key: [2; 32], size, duration_ms, waveform: vec![9; bars] }
}

// KIND: unit (group voice) — a voice post is a post (day doc, judged per post), carries the 1:1
// voice format, and its declared size, duration and waveform are bounded on every device. Posts
// written before voice posts existed still decode: `Voice` is a new variant at the end.
#[test]
fn voice_posts_are_bounded() {
    let a = acct();
    let post = |v: VoiceRef| Body::Voice { reply_to: None, voice: v };
    assert!(post(voice(1, 1, 0)).is_post() && Body::Post { text: "x".into(), reply_to: None, image: None }.is_post());
    assert!(!Body::Leave.is_post());
    for ok in [post(voice(48_000, 2_000, 64)), post(voice(MAX_VOICE_BYTES, MAX_VOICE_MS, 0)), Body::Voice { reply_to: Some("ab".into()), voice: voice(1, 1, 1) }] {
        let r = rec(&a, "g", ok);
        assert_eq!(open_record(&r.sign(&a).to_bytes(), "g").unwrap(), r);
    }
    for bad in [
        post(voice(0, 1_000, 64)),
        post(voice(MAX_VOICE_BYTES + 1, 1_000, 64)),
        post(voice(10, 0, 64)),
        post(voice(10, MAX_VOICE_MS + 1, 64)),
        post(voice(10, 1_000, MAX_VOICE_BARS + 1)),
        Body::Voice { reply_to: Some("x".repeat(65)), voice: voice(10, 1_000, 64) },
    ] {
        assert_eq!(open_record(&forge(&rec(&a, "g", bad), &a).to_bytes(), "g"), Err(RecordError::Malformed));
    }
    // The blob as the image runtime keeps it.
    let b = voice(10, 1_000, 64).as_blob();
    assert_eq!((b.hash, b.key, b.size, b.mime.as_str()), ([1; 32], [2; 32], 10, VOICE_MIME));
    // Variant indices are the wire tags: `Post` keeps 3 and `Voice` comes after `AdminsOnly`
    // (13), so records written before voice posts existed decode unchanged.
    let tag = |b: &Body| cryptography::codec::to_bytes(b).unwrap()[0];
    assert_eq!(tag(&Body::Post { text: "x".into(), reply_to: None, image: None }), 3);
    assert_eq!(tag(&Body::AdminsOnly { active: true }), 13);
    assert_eq!(tag(&post(voice(1, 1, 0))), 14);
}

fn file(size: u64, name: &str) -> FileRef {
    FileRef { hash: [1; 32], key: [2; 32], size, name: name.into(), mime: "application/pdf".into() }
}

// KIND: unit (group files) — a file post is a post (day doc, judged per post) whose size (at most
// 2 GiB), name (one path component, 1..=255 bytes) and type are bounded on every device; a holder
// announcement names a real day and a post id. Both are new variants at the end (wire tags 15
// and 16), so older records decode unchanged.
#[test]
fn file_posts_are_bounded() {
    let a = acct();
    let post = |f: FileRef| Body::File { caption: String::new(), reply_to: None, file: f };
    assert!(post(file(1, "a")).is_post() && post(file(1, "a")).in_day_doc());
    let held = |post: &str, day: &str| Body::FileHeld { post: post.into(), day: day.into(), hash: [1; 32], held: true };
    assert!(!held("p", "2026-10-30").is_post() && held("p", "2026-10-30").in_day_doc());
    assert_eq!(held("p", "2026-10-30").held_day(), Some("2026-10-30"));
    assert_eq!(MAX_FILE_BYTES, 2 * 1024 * 1024 * 1024);
    for ok in [
        post(file(0, "empty.txt")),
        post(file(MAX_FILE_BYTES, &"n".repeat(MAX_FILE_NAME))),
        post(file(5, "résumé (final).pdf")),
        Body::File { caption: "see this".into(), reply_to: Some("ab".into()), file: file(10, ".hidden") },
        held("0123456789abcdef0123456789abcdef", "2026-10-30"),
    ] {
        let r = rec(&a, "g", ok);
        assert_eq!(open_record(&r.sign(&a).to_bytes(), "g").unwrap(), r);
    }
    let mut bad_mime = file(10, "a");
    bad_mime.mime = "x".repeat(MAX_FILE_MIME + 1);
    let mut ctl_mime = file(10, "a");
    ctl_mime.mime = "text/plain\n".into();
    for bad in [
        post(file(MAX_FILE_BYTES + 1, "big.bin")),
        post(file(1, "")),
        post(file(1, &"n".repeat(MAX_FILE_NAME + 1))),
        post(file(1, "../etc/passwd")),
        post(file(1, "dir/name")),
        post(file(1, "dir\\name")),
        post(file(1, "..")),
        post(file(1, ".")),
        post(file(1, "nul\0byte")),
        post(file(1, "line\nbreak")),
        post(bad_mime),
        post(ctl_mime),
        Body::File { caption: "x".repeat(MAX_TEXT + 1), reply_to: None, file: file(1, "a") },
        Body::File { caption: String::new(), reply_to: Some("x".repeat(65)), file: file(1, "a") },
        held("", "2026-10-30"),
        held(&"p".repeat(65), "2026-10-30"),
        held("p", "2026-13-40"),
        held("p", "yesterday"),
    ] {
        assert_eq!(open_record(&forge(&rec(&a, "g", bad.clone()), &a).to_bytes(), "g"), Err(RecordError::Malformed), "{bad:?}");
    }
    let f = file(HOST_KEEP_FILE_MAX, "a");
    assert!(f.kept_by_hosts() && file(MAX_FILE_BYTES, "a").kept_by_hosts(), "option B: hosts keep every file");
    let b = f.as_blob();
    assert_eq!((b.hash, b.key, b.size, b.mime.as_str()), (f.hash, f.key, f.size, FILE_BLOB_MIME));
    let tag = |b: &Body| cryptography::codec::to_bytes(b).unwrap()[0];
    assert_eq!(tag(&post(file(1, "a"))), 15);
    assert_eq!(tag(&held("p", "2026-10-30")), 16);
}

// KIND: unit (group files, older apps) — what an app that predates a kind does with it: a
// record whose body variant it does not know is an error (never a panic), so its doc refuses
// the batch (U22). Simulated with a tag past every variant this build knows.
#[test]
fn unknown_body_kind_is_refused_not_a_panic() {
    let a = acct();
    let mut body = cryptography::codec::to_bytes(&rec(&a, "g", Body::Leave)).unwrap();
    let last = body.len() - 1;
    assert_eq!(body[last], 2, "Leave is a one-byte unit variant at the end");
    body[last] = 99;
    let sig = a.sign(&cryptography::sign::input(Record::DOMAIN, &[&body]));
    let bytes = Signed::<Record>::from_parts(body, sig).to_bytes();
    assert!(matches!(open_record(&bytes, "g"), Err(RecordError::Undecodable | RecordError::Malformed)));
    assert!(decode_trusted(&bytes, "g").is_err());
}
