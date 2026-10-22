//! `export_since_before`: an upload that leaves out a post the host would refuse.

use super::*;

// KIND: unit (public-group follow-up, Fix B) — cutting at our second post sends only the first
// (and others' posts), never the cut one or anything after it under our writer; a peer that
// already holds everything before the cut gets nothing; no cut is plain `export_since`.
#[test]
fn an_upload_leaves_out_the_cut_post_and_what_follows() {
    let (a, b) = (acct(), acct());
    let (deva, devb) = ([1; 32], [2; 32]);
    let da = dd("g", writer_id(a.did(), &deva));
    let db = dd("g", writer_id(b.did(), &devb));
    let (e1, _) = da.write(&a, &post(&a, deva, &da, "one")).unwrap();
    let (e2, _) = da.write(&a, &post(&a, deva, &da, "two")).unwrap();
    da.write(&a, &post(&a, deva, &da, "three")).unwrap();
    let (eb, ub) = db.write(&b, &post(&b, devb, &db, "theirs")).unwrap();
    let mut da = da;
    da.apply_remote(&ub, ok).unwrap();
    let cut = [(e2.peer, e2.counter)];
    let host = &mut dd("g", 99);
    let up = da.export_since_before(&host.vv(), &cut).unwrap();
    let got: Vec<String> = host
        .apply_remote(&up, ok)
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert!(got.contains(&e1.id) && got.contains(&eb.id));
    assert_eq!(got.len(), 2, "neither the cut post nor the one after it");
    assert!(
        da.export_since_before(&host.vv(), &cut).is_none(),
        "nothing left to send"
    );
    assert!(
        da.export_since_before(&host.vv(), &[]).is_some(),
        "without a cut the rest goes"
    );
}
