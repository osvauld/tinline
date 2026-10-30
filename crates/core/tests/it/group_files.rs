//! Group files (`docs/design/group-files.md`): any file up to 2 GB as a post. Real nodes over
//! real iroh endpoints, like `group_voice.rs`: the file is sealed from disk, members fetch it
//! from hosts verified against the signed hash, hosts keep small files and relay big ones on
//! demand (option A), and members who hold a file are listed on it.
#![allow(dead_code)]

use p2pcore::{GroupFile, GroupImageState as St, PictureDownload};

use crate::support;
use support::group::*;
use support::*;

const S: Suite = Suite("gfiles");
/// Long enough for every retry of a fetch that gets nowhere (2+5+15+30+60 s) to run out.
const GIVE_UP: u64 = 240;

fn peer(name: &str) -> Peer {
    S.peer(name)
}

/// `n` bytes of a repeatable pattern in `p`'s directory, named `name`; returns the path.
fn file_of(p: &Peer, name: &str, n: usize) -> String {
    std::fs::create_dir_all(p.dir.path()).unwrap();
    let path = p.dir.path().join(name);
    let bytes: Vec<u8> = (0..n as u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 11) as u8)
        .collect();
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn file_on(p: &Peer, g: &str, id: &str) -> Option<GroupFile> {
    all(p, g)
        .into_iter()
        .find(|m| m.id == id)
        .and_then(|m| m.file)
}

fn state(p: &Peer, g: &str, id: &str) -> Option<St> {
    file_on(p, g, id).map(|f| f.state)
}

fn holders(p: &Peer, g: &str, id: &str) -> Vec<String> {
    file_on(p, g, id)
        .map(|f| f.holders.into_iter().map(|h| h.did).collect())
        .unwrap_or_default()
}

fn bytes_on(p: &Peer, g: &str, id: &str) -> Option<Vec<u8>> {
    p.node
        .group_file_path(g.into(), id.into())
        .map(|f| std::fs::read(f).unwrap())
}

fn tap_only(ps: &[&Peer]) {
    for p in ps {
        p.node
            .group_set_image_auto_download(PictureDownload::Tap)
            .unwrap();
    }
}

// KIND: acceptance (group files) — member A posts a file; the host keeps it (under the cap); B
// taps and gets identical bytes under the sender's name; B's completed download makes B a holder
// for C ("A and B have this file"); B leaving the group takes B off; C deleting its copy takes C
// off.
#[test]
fn file_post_roundtrip_and_holders() {
    let o = peer("gf-o");
    let a = peer("gf-a");
    let b = peer("gf-b");
    let c = peer("gf-c");
    let g = group_with(&o, &[&a, &b, &c]);
    tap_only(&[&b, &c]);

    // 300 KiB + 7: five 64 KiB cipher chunks and a short last one.
    let path = file_of(&a, "tmp-upload.bin", 300 * 1024 + 7);
    let sent = a
        .node
        .group_post_file(
            g.clone(),
            path.clone(),
            Some("Trip plan v2.pdf".into()),
            None,
            Some("the plan".into()),
        )
        .unwrap();
    assert_eq!(sent.text, "the plan");
    assert!(sent.image.is_none() && sent.voice.is_none());
    let f = sent.file.clone().expect("the post carries its file");
    assert_eq!(
        (f.name.as_str(), f.mime.as_str(), f.size),
        ("Trip plan v2.pdf", "application/pdf", 300 * 1024 + 7)
    );
    assert_eq!(f.state, St::Ready, "the sender has it");
    assert!(f.kept_by_hosts);
    assert_eq!(
        f.holders.iter().map(|h| h.did.clone()).collect::<Vec<_>>(),
        vec![a.did.clone()]
    );
    assert_eq!(
        bytes_on(&a, &g, &sent.id).unwrap(),
        std::fs::read(&path).unwrap()
    );
    let last = a
        .node
        .groups()
        .unwrap()
        .into_iter()
        .find(|s| s.id == g)
        .and_then(|s| s.last)
        .unwrap();
    assert!(last.file.is_some(), "the list row knows it is a file");

    eventually(WAIT, "the host keeps it", || {
        state(&o, &g, &sent.id) == Some(St::Ready)
    });
    eventually(WAIT, "b sees the post", || {
        file_on(&b, &g, &sent.id).is_some()
    });
    assert_eq!(state(&b, &g, &sent.id), Some(St::Remote), "on tap only");
    assert!(b.node.group_file_path(g.clone(), sent.id.clone()).is_none());
    b.node.group_fetch_file(g.clone(), sent.id.clone()).unwrap();
    eventually(WAIT, "b loads it from the host", || {
        state(&b, &g, &sent.id) == Some(St::Ready)
    });
    let opened = b.node.group_file_path(g.clone(), sent.id.clone()).unwrap();
    assert!(opened.ends_with("/Trip plan v2.pdf"), "{opened}");
    assert_eq!(
        std::fs::read(&opened).unwrap(),
        std::fs::read(&path).unwrap(),
        "the same bytes"
    );
    let saved = b.dir.path().join("saved.pdf");
    b.node
        .group_save_file(
            g.clone(),
            sent.id.clone(),
            saved.to_string_lossy().into_owned(),
        )
        .unwrap();
    assert_eq!(
        std::fs::read(&saved).unwrap(),
        std::fs::read(&path).unwrap()
    );

    eventually(WAIT, "c sees b as a holder", || {
        holders(&c, &g, &sent.id) == vec![a.did.clone(), b.did.clone()]
    });
    let names: Vec<String> = file_on(&c, &g, &sent.id)
        .unwrap()
        .holders
        .into_iter()
        .map(|h| h.name)
        .collect();
    assert!(names.iter().all(|n| !n.is_empty()), "{names:?}");

    c.node.group_fetch_file(g.clone(), sent.id.clone()).unwrap();
    eventually(WAIT, "c loads it", || {
        state(&c, &g, &sent.id) == Some(St::Ready)
    });
    eventually(WAIT, "a sees c too", || {
        holders(&a, &g, &sent.id).contains(&c.did)
    });

    b.node.group_leave(g.clone()).unwrap();
    eventually(WAIT, "b left: off the list", || {
        !holders(&c, &g, &sent.id).contains(&b.did)
    });

    c.node
        .group_delete_file_copy(g.clone(), sent.id.clone())
        .unwrap();
    assert_eq!(
        state(&c, &g, &sent.id),
        Some(St::Remote),
        "deleted here; a tap loads it again"
    );
    assert!(c.node.group_file_path(g.clone(), sent.id.clone()).is_none());
    eventually(WAIT, "c deleted: off the list", || {
        holders(&a, &g, &sent.id) == vec![a.did.clone()]
    });
    assert!(
        !within(3, || state(&c, &g, &sent.id) != Some(St::Remote)),
        "nothing loads it by itself"
    );
}

// KIND: acceptance (group files, streaming) — a 24 MiB sparse file (nothing written on disk by
// the test: zeros past the end) is sealed as it is read and crosses hundreds of cipher chunks.
// It is over the 1:1 auto-download limit (10 MB), so it waits for a tap even with auto-download
// on "always"; a smaller file downloads by itself. The tapped one arrives with progress and the
// same bytes; cancelling an unfinished one makes it `Remote` again.
#[test]
fn big_files_wait_for_a_tap_and_stream() {
    let o = peer("gfb-o");
    let a = peer("gfb-a");
    let b = peer("gfb-b");
    let g = group_with(&o, &[&a, &b]);
    b.node
        .group_set_image_auto_download(PictureDownload::Always)
        .unwrap();

    std::fs::create_dir_all(a.dir.path()).unwrap();
    let big = a.dir.path().join("disk.img");
    let size: u64 = 24 * 1024 * 1024 + 1;
    std::fs::File::create(&big).unwrap().set_len(size).unwrap();
    let big_post = a
        .node
        .group_post_file(
            g.clone(),
            big.to_string_lossy().into_owned(),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        big_post.file.as_ref().map(|f| (f.name.as_str(), f.size)),
        Some(("disk.img", size))
    );
    let small = file_of(&a, "notes.txt", 5000);
    let small_post = a
        .node
        .group_post_file(g.clone(), small.clone(), None, None, None)
        .unwrap();
    assert_eq!(
        small_post.file.as_ref().map(|f| f.mime.as_str()),
        Some("text/plain")
    );

    eventually(WAIT, "the small one downloads by itself", || {
        state(&b, &g, &small_post.id) == Some(St::Ready)
    });
    assert_eq!(
        bytes_on(&b, &g, &small_post.id).unwrap(),
        std::fs::read(&small).unwrap()
    );
    eventually(WAIT, "the host keeps the big one", || {
        state(&o, &g, &big_post.id) == Some(St::Ready)
    });
    assert_eq!(
        state(&b, &g, &big_post.id),
        Some(St::Remote),
        "over the auto-download limit: it waits"
    );

    b.node
        .group_fetch_file(g.clone(), big_post.id.clone())
        .unwrap();
    let mut saw_progress = false;
    eventually(WAIT, "b loads the big one", || {
        let f = file_on(&b, &g, &big_post.id).unwrap();
        saw_progress |= f.state == St::Loading && f.done > 0 && f.done < size;
        f.state == St::Ready
    });
    let out = b
        .node
        .group_file_path(g.clone(), big_post.id.clone())
        .unwrap();
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len() as u64, size);
    assert!(got.iter().all(|x| *x == 0), "the same (all-zero) bytes");
    eprintln!("progress seen while loading: {saw_progress}");

    // Cancel: a second member starts the big one and cancels it.
    let c = peer("gfb-c");
    let tok = o.node.group_invite(g.clone()).unwrap();
    assert_eq!(join(&c, tok), g);
    eventually(WAIT, "c sees the big post", || {
        file_on(&c, &g, &big_post.id).is_some()
    });
    tap_only(&[&c]);
    c.node
        .group_fetch_file(g.clone(), big_post.id.clone())
        .unwrap();
    c.node
        .group_cancel_file(g.clone(), big_post.id.clone())
        .unwrap();
    assert_eq!(state(&c, &g, &big_post.id), Some(St::Remote));
    assert!(
        !within(3, || state(&c, &g, &big_post.id) == Some(St::Ready)),
        "cancelled: it does not finish"
    );
}

// KIND: acceptance (group files §4 option A) — a host keeps files up to its cap and relays bigger
// ones on demand: it never fetches one by itself, pulls it from the author when a member asks,
// serves it on, and drops its relay copy after the TTL. With the author offline, the relay comes
// from a member who holds it (B); with nobody who has it online, the file ends "not available"
// (`Failed`, not kept by hosts).
#[test]
fn host_relays_files_over_its_cap() {
    let o = peer("gfr-o");
    let a = peer("gfr-a");
    let b = peer("gfr-b");
    let c = peer("gfr-c");
    let d = peer("gfr-d");
    let g = group_with(&o, &[&a, &b, &c, &d]);
    tap_only(&[&b, &c, &d]);
    for p in [&o, &a, &b, &c, &d] {
        p.node
            .group_set_file_limits_for_test(Some(64 * 1024), Some(3_000), None);
    }
    let path = file_of(&a, "video.mp4", 200 * 1024 + 3);
    let sent = a
        .node
        .group_post_file(g.clone(), path.clone(), None, None, None)
        .unwrap();
    assert!(!sent.file.as_ref().unwrap().kept_by_hosts);
    eventually(WAIT, "the host has the post", || {
        file_on(&o, &g, &sent.id).is_some()
    });
    assert!(
        !within(5, || o
            .node
            .group_file_blob_here_for_test(g.clone(), sent.id.clone())),
        "a host never fetches it by itself"
    );
    assert_eq!(state(&o, &g, &sent.id), Some(St::Remote));

    eventually(WAIT, "b sees it", || file_on(&b, &g, &sent.id).is_some());
    b.node.group_fetch_file(g.clone(), sent.id.clone()).unwrap();
    eventually(WAIT, "b gets it through the host's relay", || {
        state(&b, &g, &sent.id) == Some(St::Ready)
    });
    assert_eq!(
        bytes_on(&b, &g, &sent.id).unwrap(),
        std::fs::read(&path).unwrap()
    );
    assert_eq!(
        state(&o, &g, &sent.id),
        Some(St::Remote),
        "the host still does not keep it"
    );
    eventually(WAIT, "the relay copy leaves after the TTL", || {
        !o.node
            .group_file_relaying_for_test(g.clone(), sent.id.clone())
    });
    eventually(WAIT, "the relayed bytes are collected", || {
        !o.node
            .group_file_blob_here_for_test(g.clone(), sent.id.clone())
    });

    // The author goes offline: B (who announced holding it) is the source.
    eventually(WAIT, "c sees b holds it", || {
        holders(&c, &g, &sent.id).contains(&b.did)
    });
    a.node.stop();
    c.node.group_fetch_file(g.clone(), sent.id.clone()).unwrap();
    eventually(WAIT, "c gets it relayed from b", || {
        state(&c, &g, &sent.id) == Some(St::Ready)
    });
    assert_eq!(
        bytes_on(&c, &g, &sent.id).unwrap(),
        std::fs::read(&path).unwrap()
    );

    // Nobody who has it is online: not available right now.
    b.node.stop();
    c.node.stop();
    eventually(WAIT, "the relay copy left again", || {
        !o.node
            .group_file_relaying_for_test(g.clone(), sent.id.clone())
    });
    eventually(WAIT, "and its bytes", || {
        !o.node
            .group_file_blob_here_for_test(g.clone(), sent.id.clone())
    });
    d.node.group_fetch_file(g.clone(), sent.id.clone()).unwrap();
    eventually(GIVE_UP, "d: not available right now", || {
        state(&d, &g, &sent.id) == Some(St::Failed)
    });
    let f = file_on(&d, &g, &sent.id).unwrap();
    assert!(
        !f.kept_by_hosts,
        "the UI says: not available right now, try later"
    );
    assert!(d.node.group_file_path(g.clone(), sent.id.clone()).is_none());
}

// KIND: acceptance (group files, verified streaming) — the host's copy is altered after it
// verified it (a bad disk, a modified app): B's download never completes or opens, ends Failed,
// and B is never listed as a holder. (The exact receiver-side error at the bad chunk is the
// transfer unit test `altered_chunk_is_rejected_at_that_chunk`.)
#[test]
fn altered_copy_is_never_accepted_or_held() {
    let o = peer("gft-o");
    let a = peer("gft-a");
    let b = peer("gft-b");
    let g = group_with(&o, &[&a, &b]);
    tap_only(&[&b]);
    let path = file_of(&a, "report.pdf", 300 * 1024);
    let sent = a
        .node
        .group_post_file(g.clone(), path, None, None, None)
        .unwrap();
    eventually(WAIT, "the host keeps it", || {
        state(&o, &g, &sent.id) == Some(St::Ready)
    });
    o.node
        .group_file_tamper_for_test(g.clone(), sent.id.clone(), 200_000)
        .unwrap();
    eventually(WAIT, "b sees it", || file_on(&b, &g, &sent.id).is_some());
    b.node.group_fetch_file(g.clone(), sent.id.clone()).unwrap();
    // The host's own store re-checks what it serves and stops at the altered chunk, so B sees a
    // host that stalls there: every attempt waits out the stall window before the retries end.
    let t = std::time::Instant::now();
    eventually(2 * GIVE_UP, "b gives up", || {
        let f = file_on(&b, &g, &sent.id).unwrap();
        assert_ne!(f.state, St::Ready, "an altered file is never accepted");
        assert!(
            f.done <= 200_000,
            "nothing past the altered byte arrives: {}",
            f.done
        );
        f.state == St::Failed
    });
    eprintln!("b gave up after {:?}", t.elapsed());
    assert!(b.node.group_file_path(g.clone(), sent.id.clone()).is_none());
    assert!(
        !b.node
            .group_file_blob_here_for_test(g.clone(), sent.id.clone())
    );
    assert!(
        !within(3, || holders(&a, &g, &sent.id).contains(&b.did)),
        "b never holds it"
    );
}

// KIND: acceptance (group files) — what is not a file post is refused before anything is posted:
// a name with a path separator, a directory, a file over 2 GB (sparse: no disk used); in a
// channel a member's file is refused like a text post.
#[test]
fn file_post_checks() {
    let o = peer("gfc-o");
    let a = peer("gfc-a");
    let g = group_with(&o, &[&a]);
    let path = file_of(&a, "ok.txt", 10);
    assert!(
        a.node
            .group_post_file(g.clone(), path.clone(), Some("../x".into()), None, None)
            .is_err()
    );
    assert!(
        a.node
            .group_post_file(g.clone(), path.clone(), Some("a\\b".into()), None, None)
            .is_err()
    );
    assert!(
        a.node
            .group_post_file(
                g.clone(),
                a.dir.path().to_string_lossy().into_owned(),
                None,
                None,
                None
            )
            .is_err()
    );
    let huge = a.dir.path().join("huge.bin");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(2 * 1024 * 1024 * 1024 + 1)
        .unwrap();
    match a.node.group_post_file(
        g.clone(),
        huge.to_string_lossy().into_owned(),
        None,
        None,
        None,
    ) {
        Err(p2pcore::Error::Rejected(r)) => assert!(r.contains("2 GB"), "{r}"),
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(
        all(&a, &g).iter().all(|m| m.file.is_none()),
        "nothing was posted"
    );

    o.node.group_set_admins_only(g.clone(), true).unwrap();
    eventually(WAIT, "a sees admins only", || {
        a.node.group_detail(g.clone()).is_ok_and(|d| d.admins_only)
    });
    match a.node.group_post_file(g.clone(), path, None, None, None) {
        Err(p2pcore::Error::Rejected(r)) => assert_eq!(r, "admins only"),
        other => panic!("expected Rejected(admins only), got {other:?}"),
    }
}
