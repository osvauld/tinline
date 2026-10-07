use super::blobs::*;
use super::crypt;
use iroh::endpoint::presets;
use iroh::protocol::ProtocolHandler;
use iroh::{Endpoint, SecretKey};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tl-blobs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn endpoint(alpns: Vec<Vec<u8>>) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .alpns(alpns)
        .bind()
        .await
        .unwrap()
}

/// `a` serves, with the gate answering `allow` for `b`'s device only; `b` dials `a`.
async fn setup(allow: Arc<AtomicBool>, tag: &str) -> (BlobHub, BlobHub, Endpoint, Endpoint, PathBuf) {
    let dir = tmpdir(tag);
    let a_ep = endpoint(vec![iroh_blobs::ALPN.to_vec()]).await;
    let b_ep = endpoint(vec![]).await;
    let b_id = *b_ep.id().as_bytes();
    let gate: Gate = Arc::new(move |dev, _| *dev == b_id && allow.load(Ordering::SeqCst));
    let a = BlobHub::open(&dir.join("a"), gate, Arc::new(|_, _, _, _| {})).await.unwrap();
    let deny: Gate = Arc::new(|_, _| false);
    let b = BlobHub::open(&dir.join("b"), deny, Arc::new(|_, _, _, _| {})).await.unwrap();
    let proto = a.protocol();
    let ep = a_ep.clone();
    tokio::spawn(async move {
        while let Some(inc) = ep.accept().await {
            let proto = proto.clone();
            tokio::spawn(async move {
                if let Ok(conn) = inc.await {
                    let _ = proto.accept(conn).await;
                }
            });
        }
    });
    (a, b, a_ep, b_ep, dir)
}

#[tokio::test(flavor = "multi_thread")]
async fn encrypted_blob_round_trip_and_gate() {
    let allow = Arc::new(AtomicBool::new(true));
    let (a, b, a_ep, b_ep, dir) = setup(allow.clone(), "rt").await;
    let plain: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    let src = dir.join("plain.bin");
    std::fs::write(&src, &plain).unwrap();
    let key = crypt::new_key();
    let (hash, size) = a.add_file(key, src, |_| {}).await.unwrap();
    assert_eq!(size, plain.len() as u64);
    assert_eq!(a.complete_size(&hash).await, Some(crypt::cipher_len(size)));
    // The store never holds plaintext.
    let stored = a.read_cipher(&hash).await.unwrap();
    assert!(!stored.windows(64).any(|w| w == &plain[1000..1064]));

    // A party fetches, decrypts to a file, and gets identical bytes.
    let conn = b_ep.connect(a_ep.addr(), iroh_blobs::ALPN).await.unwrap();
    let mut last = 0;
    b.fetch(conn.clone(), hash, size, |n, _| last = n).await.unwrap();
    assert_eq!(last, size);
    let out = dir.join("out.bin");
    b.export_plain(hash, key, out.clone(), |_| {}).await.unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), plain);

    // A wrong key does not open it.
    let bad = dir.join("bad.bin");
    assert!(b.export_plain(hash, crypt::new_key(), bad.clone(), |_| {}).await.is_err());
    assert!(!bad.exists());

    // A device the gate refuses cannot fetch (fresh store, no local copy).
    allow.store(false, Ordering::SeqCst);
    let c = BlobHub::open(&dir.join("c"), Arc::new(|_, _| false), Arc::new(|_, _, _, _| {})).await.unwrap();
    let conn2 = b_ep.connect(a_ep.addr(), iroh_blobs::ALPN).await.unwrap();
    assert!(c.fetch(conn2, hash, size, |_, _| {}).await.is_err());
    assert_eq!(c.complete_size(&hash).await, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn released_blobs_are_collected() {
    let dir = tmpdir("gc");
    let a = BlobHub::open(&dir.join("a"), Arc::new(|_, _| true), Arc::new(|_, _, _, _| {})).await.unwrap();
    let hash = a.add_bytes(crypt::new_key(), vec![7u8; 5000]).await.unwrap();
    assert!(a.complete_size(&hash).await.is_some());
    a.release(&hash).await.unwrap();
    let t = std::time::Instant::now();
    while a.complete_size(&hash).await.is_some() && t.elapsed() < GC_INTERVAL * 3 {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(a.complete_size(&hash).await.is_none(), "blob should be gone after GC");
    let _ = std::fs::remove_dir_all(&dir);
}
