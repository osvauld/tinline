//! Pairs, writer IDs, signatures and record fields.

use super::*;

// KIND: invariant
#[test]
fn pairs() {
    // Days: `crdt::time` tests.
    assert_eq!(pair_id("did:a", "did:b"), pair_id("did:b", "did:a"));
    assert_ne!(pair_id("did:a", "did:b"), pair_id("did:a", "did:c"));
}

// KIND: invariant
#[test]
fn writer_ids_are_domain_separated_and_derived_from_no_device_key() {
    // Deterministic, and it changes with the DID and with the device ID.
    assert_eq!(wid(0, 1), writer_id(&did(0), &val(1)));
    assert_ne!(wid(0, 1), wid(1, 1));
    assert_ne!(wid(0, 1), wid(0, 2));
    // The DID is length-prefixed: moving bytes between DID and device ID gives another ID.
    let mut v = [0u8; 32];
    v[0] = b'b';
    let mut x = [0u8; 32];
    x[..31].copy_from_slice(&v[1..]);
    assert_ne!(writer_id("a", &v), writer_id("ab", &x));
    // Its own domain, not the plain hash.
    let h = blake3::hash(&[did(0).as_bytes(), &val(1)].concat());
    assert_ne!(wid(0, 1), u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap()));
    // A writer writes under it.
    assert_eq!(w(0, 1).writer_id(), wid(0, 1));
    // Fresh device IDs are random.
    assert_ne!(new_device_value(), new_device_value());
}

// KIND: invariant
#[test]
fn did_signatures_are_domain_separated_and_bound_to_doc_content_device_and_span() {
    let rec = rec_of("m1", 0, 1, NOON, "x");
    let at = WrittenAt { peer: wid(0, 1), counter: 3, len: 4 };
    let s = sign_msg(acct(0), &name(), &rec, &at);
    let signed = MsgRec { did_sig: s.clone(), ..rec.clone() };
    assert!(verify_msg(&name(), &signed, &at));
    assert!(!verify_msg(&doc_name("q", DAY), &signed, &at));
    assert!(!verify_msg(&name(), &MsgRec { text: "y".into(), ..signed.clone() }, &at));
    assert!(!verify_msg(&name(), &MsgRec { author: did(1), ..signed.clone() }, &at));
    // Bound to the device ID and to the span that writes the version.
    assert!(!verify_msg(&name(), &MsgRec { dev: val(2), ..signed.clone() }, &at));
    for other in [WrittenAt { peer: at.peer ^ 1, ..at }, WrittenAt { counter: 4, ..at }, WrittenAt { len: 5, ..at }] {
        assert!(!verify_msg(&name(), &signed, &other));
    }
    // Another account's key, a device key over the same bytes, or the P9b domain: no.
    assert!(!verify_msg(&name(), &MsgRec { did_sig: sign_msg(acct(1), &name(), &rec, &at), ..rec.clone() }, &at));
    let device = proto::new_device_secret();
    let by_device = cryptography::signature::sign(&device, &msg_sig_input(&name(), &rec, &at)).to_vec();
    assert!(!verify_msg(&name(), &MsgRec { did_sig: by_device, ..rec.clone() }, &at));
    let mut p9b = b"tinline-chat-msg-did-v2".to_vec();
    let input = msg_sig_input(&name(), &rec, &at);
    p9b.extend_from_slice(&input[b"tinline-chat-msg-v3".len()..]);
    assert!(!verify_msg(&name(), &MsgRec { did_sig: acct(0).sign(&p9b).to_vec(), ..rec.clone() }, &at));
    // An author that is not a `did:key` never verifies.
    assert!(!verify_msg(&name(), &MsgRec { author: "A".into(), ..signed }, &at));
}

// KIND: invariant (P9c)
// The DID signature and the device ID are record fields (of the version value the writer files
// under its own version key, `v:<writer ID>`); the record has no device signature and no change
// carries a commit message. The writer ID is the only
// peer id in the doc.
#[test]
fn records_carry_the_did_signature_and_device_id_as_fields() {
    let a = shard(0, 1);
    let u = a.add_message(&msg("m1", &did(0), NOON, "hi"), &w(0, 1)).unwrap();
    a.edit("m1", "hi!", NOON + 10, &w(0, 1)).unwrap();
    assert_eq!(a.peer(), wid(0, 1));
    assert_eq!(peers_of(&u), vec![wid(0, 1)]);
    let doc = LoroDoc::new();
    doc.import(&a.snapshot()).unwrap();
    let loro::LoroValue::Map(top) = doc.get_deep_value() else { panic!() };
    let loro::LoroValue::Map(msgs) = &top["messages"] else { panic!() };
    let loro::LoroValue::Map(m1) = &msgs["m1"] else { panic!() };
    let mut keys: Vec<&str> = m1.keys().map(|k| k.as_str()).collect();
    keys.sort();
    let vk = version_key(wid(0, 1));
    assert_eq!(keys, vec!["at", "author", "id", vk.as_str()]);
    let loro::LoroValue::Map(v) = &m1[&vk] else { panic!() };
    let mut keys: Vec<&str> = v.keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(keys, vec!["deleted", "dev", "did_sig", "edited_at", "text"]);
    let rec = &a.messages().unwrap()["m1"];
    assert_eq!(rec.dev, val(1));
    let json = a.export_all_since(&loro::VersionVector::default()).unwrap();
    let mut b = recv();
    b.apply_remote(&json, &hist(0, 1)).unwrap();
    let changes = {
        let d = LoroDoc::new();
        d.import(&json).unwrap();
        d.export_json_updates_without_peer_compression(&Default::default(), &d.oplog_vv()).changes
    };
    assert!(changes.iter().all(|c| c.msg.is_none()), "no commit messages");
    assert_eq!(b.messages().unwrap()["m1"].text, "hi!");
}

// KIND: invariant (chat/5: a photo's size and preview are part of the author proof)
#[test]
fn photo_size_and_preview_are_signed_and_older_records_keep_their_input() {
    let file = FileRef {
        hash: "h".into(), key: "k".into(), name: "p.jpg".into(), size: 10, mime: "image/jpeg".into(), kind: "file".into(),
        duration_ms: 0, waveform: vec![], width: 0, height: 0, preview: String::new(),
    };
    let at = WrittenAt { peer: wid(0, 1), counter: 3, len: 4 };
    // A record without picture fields signs exactly what it did before /5 (no trailing bytes after the waveform).
    let old = MsgRec { file: Some(file.clone()), ..rec_of("m1", 0, 1, NOON, "") };
    let input = msg_sig_input(&name(), &old, &at);
    let tail = [&0u32.to_be_bytes()[..], &old.dev[..]].concat();
    assert!(input.windows(tail.len()).any(|w| w == tail.as_slice()), "the waveform length is followed directly by the device id");
    // With a size and preview, every one of them is covered.
    let pic = FileRef { width: 4000, height: 3000, preview: "aa".repeat(36), ..file };
    let rec = MsgRec { file: Some(pic.clone()), ..old.clone() };
    let signed = MsgRec { did_sig: sign_msg(acct(0), &name(), &rec, &at), ..rec.clone() };
    assert!(verify_msg(&name(), &signed, &at));
    for f in [FileRef { width: 3000, ..pic.clone() }, FileRef { height: 4000, ..pic.clone() }, FileRef { preview: "ab".repeat(36), ..pic.clone() }] {
        assert!(!verify_msg(&name(), &MsgRec { file: Some(f), ..signed.clone() }, &at));
    }
    // Stripping them is a change too.
    assert!(!verify_msg(&name(), &MsgRec { file: Some(FileRef { width: 0, height: 0, preview: String::new(), ..pic }), ..signed }, &at));
}
