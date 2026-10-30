use super::super::record::*;
use super::*;

fn acct() -> identity::Identity {
    identity::generate().0
}

/// The day doc the tests' posts (`at: 1`) belong in.
fn day() -> DocId {
    DocId::Day("1970-01-01".into())
}

fn dd(group: &str, peer: u64) -> GroupDoc {
    GroupDoc::new_in(group, day(), peer)
}

fn post(a: &identity::Identity, dev: [u8; 32], doc: &GroupDoc, text: &str) -> Record {
    Record {
        group: doc.group.clone(),
        author: a.did().into(),
        dev,
        at: 1,
        seen: doc.seen(),
        body: Body::Post {
            text: text.into(),
            reply_to: None,
            image: None,
        },
    }
}

fn ok(_: &[Entry]) -> Result<(), String> {
    Ok(())
}

// KIND: unit (G1 stage 1)
#[test]
fn a_signed_record_round_trips_between_docs() {
    let a = acct();
    let dev = [1; 32];
    let da = dd("g", writer_id(a.did(), &dev));
    let (e, up) = da.write(&a, &post(&a, dev, &da, "hello")).unwrap();
    let b = acct();
    let mut db = dd("g", writer_id(b.did(), &[2; 32]));
    let got = db.apply_remote(&up, ok).unwrap();
    assert_eq!(got, vec![e.clone()]);
    assert_eq!(db.entries().unwrap(), vec![e]);
    // Applying again: nothing new.
    assert!(db.apply_remote(&up, ok).unwrap().is_empty());
    // Reload from snapshot.
    let re = GroupDoc::load_in("g", day(), 9, Some(&db.snapshot()), &[]).unwrap();
    assert_eq!(re.entries().unwrap().len(), 1);
}

// KIND: unit (G1 stage 1) — U10/U16: forged, tampered and foreign-peer batches are refused whole.
#[test]
fn forged_tampered_and_foreign_batches_are_refused() {
    let a = acct();
    let m = acct();
    let dev = [1; 32];
    let mut target = dd("g", 77);

    // Signed by a, claims to be m: bad signature.
    let mallory = dd("g", writer_id(a.did(), &dev));
    let mut r = post(&a, dev, &mallory, "x");
    r.author = m.did().into();
    let body = cryptography::codec::to_bytes(&r).unwrap();
    let fake = SignedRecord::from_parts(body, a.sign(b"whatever")).to_bytes();
    let up = mallory.raw_insert(&record_id(&fake), fake);
    assert!(matches!(target.apply_remote(&up, ok), Err(Reject::Record(RecordError::BadSignature))));

    // A genuine record of m's replayed under another peer id: foreign peer.
    let dm = dd("g", writer_id(m.did(), &dev));
    let (_, _) = dm.write(&m, &post(&m, dev, &dm, "mine")).unwrap();
    let genuine = dm.entries().unwrap().remove(0);
    let bytes = genuine.rec.sign(&m).to_bytes();
    let copier = dd("g", writer_id(a.did(), &dev));
    let up = copier.raw_insert(&record_id(&bytes), bytes.clone());
    assert_eq!(target.apply_remote(&up, ok), Err(Reject::ForeignPeer));

    // Key that is not the id.
    let liar = dd("g", writer_id(m.did(), &dev));
    let up = liar.raw_insert("not-the-id", bytes.clone());
    assert_eq!(target.apply_remote(&up, ok), Err(Reject::WrongKey));

    // Another group's record.
    let other = dd("h", writer_id(m.did(), &dev));
    let (_, up) = other.write(&m, &post(&m, dev, &other, "elsewhere")).unwrap();
    assert!(matches!(target.apply_remote(&up, ok), Err(Reject::Record(RecordError::WrongGroup))));

    // Deletes and other containers.
    let mut t2 = dd("g", 78);
    t2.apply_remote(&dm.export_since(&VersionVector::default()).unwrap(), ok).unwrap();
    let del = dm.raw_delete(&genuine.id);
    assert_eq!(t2.apply_remote(&del, ok), Err(Reject::Structure));
    assert_eq!(t2.entries().unwrap().len(), 1);
    let side = dd("g", writer_id(m.did(), &[3; 32]));
    let up = side.raw_other();
    assert_eq!(target.apply_remote(&up, ok), Err(Reject::Structure));

    // A good record and a bad one in one batch: nothing is imported.
    let mixed = dd("g", writer_id(m.did(), &[4; 32]));
    mixed.write(&m, &post(&m, [4; 32], &mixed, "good")).unwrap();
    let before = target.vv();
    mixed.raw_insert("junk", vec![1, 2, 3]);
    assert!(target.apply_remote(&mixed.export_since(&VersionVector::default()).unwrap(), ok).is_err());
    assert_eq!(target.vv(), before);
    assert!(target.entries().unwrap().is_empty());
}

// KIND: unit (G1 stage 1) — the policy hook refuses a batch without importing it.
#[test]
fn a_policy_refusal_imports_nothing() {
    let m = acct();
    let dm = dd("g", writer_id(m.did(), &[1; 32]));
    let (_, up) = dm.write(&m, &post(&m, [1; 32], &dm, "x")).unwrap();
    let mut t = dd("g", 5);
    assert!(matches!(t.apply_remote(&up, |_| Err("removed".into())), Err(Reject::Policy(_))));
    assert!(t.entries().unwrap().is_empty());
}

// KIND: unit (G1 stage 1) — a writer cannot write under a writer ID that is not its own.
#[test]
fn writing_under_another_writer_id_is_refused_locally() {
    let a = acct();
    let d = dd("g", writer_id(a.did(), &[1; 32]));
    let r = post(&a, [2; 32], &d, "x");
    assert_eq!(d.write(&a, &r).unwrap_err(), Reject::ForeignPeer);
    d.set_peer(writer_id(a.did(), &[2; 32]));
    assert!(d.write(&a, &r).is_ok());
}

// KIND: unit (group shards) — a post belongs only in the day doc of its day (+- 1 h); every
// other record only in meta; a wrong doc refuses the whole batch.
#[test]
fn records_are_accepted_only_in_their_doc() {
    let a = acct();
    let dev = [1; 32];
    let peer = writer_id(a.did(), &dev);
    let meta = GroupDoc::new("g", peer);
    let rec = |at: i64, body: Body| Record {
        group: "g".into(),
        author: a.did().into(),
        dev,
        at,
        seen: vec![],
        body,
    };
    let p = |at| {
        rec(
            at,
            Body::Post {
                text: "x".into(),
                reply_to: None,
                image: None,
            },
        )
    };
    let leave = rec(1, Body::Leave);
    // Local writes refuse the wrong doc too.
    assert_eq!(meta.write(&a, &p(1)).unwrap_err(), Reject::WrongDoc);
    let d0 = DocId::Day("2024-05-10".into());
    let t0 = day_start("2024-05-10").unwrap();
    let day0 = GroupDoc::new_in("g", d0.clone(), peer);
    assert_eq!(day0.write(&a, &leave).unwrap_err(), Reject::WrongDoc);
    assert_eq!(day0.write(&a, &p(t0 - 2 * HOUR_MS)).unwrap_err(), Reject::OutOfDay);
    assert_eq!(day0.write(&a, &p(t0 + DAY_MS + HOUR_MS)).unwrap_err(), Reject::OutOfDay);
    // Within the day +- 1 h.
    day0.write(&a, &p(t0 - HOUR_MS)).unwrap();
    day0.write(&a, &p(t0 + 5)).unwrap();
    day0.write(&a, &p(t0 + DAY_MS + HOUR_MS - 1)).unwrap();
    meta.write(&a, &leave).unwrap();

    // On arrival the same verdicts, whole batch.
    let forged_post = GroupDoc::new("g", peer);
    let up = forged_post.raw_insert(&record_id(&p(1).sign(&a).to_bytes()), p(1).sign(&a).to_bytes());
    let mut m2 = GroupDoc::new("g", 90);
    assert_eq!(m2.apply_remote(&up, ok), Err(Reject::WrongDoc));
    let forged_leave = GroupDoc::new_in("g", day(), peer);
    let up = forged_leave.raw_insert(&record_id(&leave.sign(&a).to_bytes()), leave.sign(&a).to_bytes());
    let mut d2 = dd("g", 91);
    assert_eq!(d2.apply_remote(&up, ok), Err(Reject::WrongDoc));
    // A good post for another day in a day doc: out of day.
    let wrong_day = GroupDoc::new_in("g", DocId::Day("2024-05-11".into()), peer);
    let good = p(t0 + DAY_MS + 2 * HOUR_MS).sign(&a).to_bytes();
    let up = wrong_day.raw_insert(&record_id(&good), good);
    let mut d3 = GroupDoc::new_in("g", d0, 92);
    assert_eq!(d3.apply_remote(&up, ok), Err(Reject::OutOfDay));
    assert!(d3.entries().unwrap().is_empty());
    // Day names are canonical calendar days.
    assert!(DocId::Day("2024-02-29".into()).is_valid() && DocId::Meta.is_valid());
    assert!(!DocId::Day("2023-02-29".into()).is_valid() && !DocId::Day("2024-5-1".into()).is_valid() && !DocId::Day("meta".into()).is_valid());
}

// KIND: unit (group shards) — our own sealed rows load without checking signatures again.
#[test]
fn trusted_entries_read_without_verifying() {
    let a = acct();
    let dev = [1; 32];
    let d = dd("g", writer_id(a.did(), &dev));
    let (e, _) = d.write(&a, &post(&a, dev, &d, "hello")).unwrap();
    let re = GroupDoc::load_in("g", day(), 5, Some(&d.snapshot()), &[]).unwrap();
    assert_eq!(re.entries_trusted().unwrap(), vec![e]);
    // A forged signature slips through the trusted read, never the verifying one.
    let mut r = post(&a, dev, &d, "forged");
    r.at = 2;
    let fake =
        SignedRecord::from_parts(cryptography::codec::to_bytes(&r).unwrap(), [0; 64]).to_bytes();
    let forger = dd("g", writer_id(a.did(), &dev));
    forger.raw_insert(&record_id(&fake), fake);
    assert_eq!(forger.entries_trusted().unwrap().len(), 1);
    assert!(forger.entries().is_err());
}

// KIND: unit (group shards) — the in-place apply of big docs: same verdicts as `apply_remote`;
// a refused batch leaves the doc tainted (the caller reloads it), a good one does not.
#[test]
fn in_place_apply_agrees_and_taints_on_refusal() {
    let a = acct();
    let dev = [1; 32];
    let src = dd("g", writer_id(a.did(), &dev));
    let (e, up) = src.write(&a, &post(&a, dev, &src, "hello")).unwrap();
    let mut t = dd("g", 5);
    assert_eq!(t.apply_remote_inplace(&up, ok).unwrap(), vec![e]);
    assert!(!t.tainted());
    assert!(t.apply_remote_inplace(&up, ok).unwrap().is_empty() && !t.tainted(), "nothing new");
    // Refused by policy, by structure and by record: tainted each time.
    let (_, up2) = src.write(&a, &post(&a, dev, &src, "two")).unwrap();
    assert!(matches!(t.apply_remote_inplace(&up2, |_| Err("no".into())), Err(Reject::Policy(_))));
    assert!(t.tainted());
    let mut u = dd("g", 6);
    let forged = dd("g", writer_id(a.did(), &[9; 32]));
    let up3 = forged.raw_insert("not-the-id", vec![1, 2, 3]);
    assert!(u.apply_remote_inplace(&up3, ok).is_err() && u.tainted());
    let mut m = GroupDoc::new("g", 7);
    assert_eq!(m.apply_remote_inplace(&up, ok), Err(Reject::WrongDoc));
    assert!(m.tainted());
}

mod upload;

// KIND: unit (group files) — a file post goes in the day doc of its time like any post; a holder
// announcement goes only in the day doc of the post it names, whenever it is written (days
// later), never in meta or another day; on arrival the same verdicts.
#[test]
fn file_holder_notes_live_in_their_posts_day() {
    let a = acct();
    let dev = [1; 32];
    let peer = writer_id(a.did(), &dev);
    let rec = |at: i64, body: Body| Record { group: "g".into(), author: a.did().into(), dev, at, seen: vec![], body };
    let t0 = day_start("2024-05-10").unwrap();
    let file = FileRef { hash: [5; 32], key: [6; 32], size: 9, name: "a.zip".into(), mime: "application/zip".into() };
    let fpost = rec(t0 + 5, Body::File { caption: String::new(), reply_to: None, file });
    let held = |day: &str| rec(t0 + 3 * DAY_MS, Body::FileHeld { post: "ab".into(), day: day.into(), hash: [5; 32], held: true });
    let day0 = GroupDoc::new_in("g", DocId::Day("2024-05-10".into()), peer);
    day0.write(&a, &fpost).unwrap();
    day0.write(&a, &held("2024-05-10")).unwrap();
    let later = GroupDoc::new_in("g", DocId::Day("2024-05-13".into()), peer);
    assert_eq!(later.write(&a, &held("2024-05-10")).unwrap_err(), Reject::WrongDoc, "not in the day of its own time");
    assert_eq!(GroupDoc::new("g", peer).write(&a, &held("2024-05-10")).unwrap_err(), Reject::WrongDoc, "not in meta");
    assert_eq!(GroupDoc::new("g", peer).write(&a, &fpost).unwrap_err(), Reject::WrongDoc);
    let bytes = held("2024-05-10").sign(&a).to_bytes();
    let up = later.raw_insert(&record_id(&bytes), bytes);
    let mut d = GroupDoc::new_in("g", DocId::Day("2024-05-13".into()), 93);
    assert_eq!(d.apply_remote(&up, ok), Err(Reject::WrongDoc));
    assert!(d.entries().unwrap().is_empty());
}
