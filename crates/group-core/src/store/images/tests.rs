use super::*;

fn h(n: u8) -> [u8; 32] {
    [n; 32]
}

fn img(n: u8) -> ImageRef {
    ImageRef {
        hash: h(n),
        key: [9; 32],
        size: 1000,
        w: 10,
        h: 10,
        mime: "image/jpeg".into(),
        preview: vec![],
    }
}

// KIND: unit (public group §5) — the host's budget drops blobs no visible post references
// first, then the oldest referenced ones, and stops as soon as the rest fits.
#[test]
fn eviction_drops_unreferenced_then_oldest() {
    // (hash, bytes, at, referenced)
    let held = [
        (h(1), 400, 10, true),
        (h(2), 400, 30, false),
        (h(3), 400, 20, true),
        (h(4), 400, 5, false),
        (h(5), 400, 40, true),
    ];
    assert!(plan_eviction(&held, 2000).is_empty(), "fits: nothing goes");
    assert_eq!(
        plan_eviction(&held, 1600),
        vec![h(4)],
        "the oldest unreferenced first"
    );
    assert_eq!(plan_eviction(&held, 1200), vec![h(4), h(2)]);
    assert_eq!(
        plan_eviction(&held, 800),
        vec![h(4), h(2), h(1)],
        "then the oldest referenced"
    );
    assert_eq!(plan_eviction(&held, 0), vec![h(4), h(2), h(1), h(3), h(5)]);
    // Ties on time go by hash, so every run agrees.
    let tie = [(h(7), 1, 1, true), (h(6), 1, 1, true)];
    assert_eq!(plan_eviction(&tie, 1), vec![h(6)]);
}

// KIND: unit (public group §5) — image rows and the post index round-trip, `forget` drops them,
// and the auto-download setting defaults to Wi-Fi only.
#[test]
fn image_rows_round_trip_and_forget() {
    let s = GroupStore::in_memory("synthetic image rows");
    assert_eq!(s.image_download(), IMG_DL_DEFAULT);
    s.set_image_download(2).unwrap();
    assert_eq!(s.image_download(), 2);
    let row = ImgRow {
        post: "p1".into(),
        author: "did:key:a".into(),
        at: 5,
        image: img(1),
        ready: false,
        wanted: false,
        failed: false,
        dropped: false,
        referenced: true,
        file: None,
        deleted: false,
    };
    s.put_image("g", &row).unwrap();
    s.index_image_post("g", "p2", &h(1)).unwrap();
    assert_eq!(s.image("g", &h(1)).unwrap(), Some(row.clone()));
    assert_eq!(s.image_of_post("g", "p2").unwrap(), Some(row.clone()));
    assert_eq!(s.images("g").unwrap(), vec![row]);
    assert!(s.image_of_post("g", "nope").unwrap().is_none());
    s.forget("g").unwrap();
    assert!(s.images("g").unwrap().is_empty());
    assert!(s.image_of_post("g", "p1").unwrap().is_none());
}
