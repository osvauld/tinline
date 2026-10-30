//! Owned file import, progress, and verified streaming against a provider that alters a chunk.

use super::*;
use crate::crypt;
use iroh::endpoint::presets;
use iroh::{Endpoint, SecretKey};
use std::sync::Arc;

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tl-verify-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn endpoint(alpns: Vec<Vec<u8>>) -> Endpoint {
    Endpoint::builder(presets::Minimal).secret_key(SecretKey::generate()).alpns(alpns).bind().await.unwrap()
}

fn hub_open(dir: &Path, allow: bool) -> impl std::future::Future<Output = Result<BlobHub, Error>> + '_ {
    BlobHub::open(dir, Arc::new(move |_, _| allow), Arc::new(|_, _, _, _| {}), Arc::new(|_, _, _| {}))
}

/// Serves every GET on `ep` with `cipher` bao-encoded against the outboard of `outboard_of`
/// (the honest blob): the tree's hashes are right, so the requester verifies up to the altered
/// chunk and fails exactly there. This is what a modified app relaying a file could send.
fn tampering_provider(ep: Endpoint, cipher: Vec<u8>, outboard_of: Vec<u8>) {
    use bao_tree::io::outboard::PreOrderMemOutboard;
    use bao_tree::{BlockSize, ChunkRanges};
    let ob = PreOrderMemOutboard::create(&outboard_of, BlockSize::from_chunk_log(4));
    let mut body = Vec::new();
    body.extend_from_slice(&(cipher.len() as u64).to_le_bytes());
    bao_tree::io::sync::encode_ranges(&cipher[..], &ob, &ChunkRanges::all(), &mut body).unwrap();
    let body = Arc::new(body);
    tokio::spawn(async move {
        while let Some(inc) = ep.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let Ok(conn) = inc.await else { return };
                while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                    let _ = recv.read_to_end(1 << 20).await;
                    let _ = send.write_all(&body).await;
                    let _ = send.finish();
                }
            });
        }
    });
}

// KIND: unit (group files) — a file is sealed from disk into an owner-tagged blob (streamed, the
// owner's claim protects it), fetched with progress across many 64 KiB cipher chunks and 16 KiB
// bao chunk groups, and opens to identical bytes.
#[tokio::test(flavor = "multi_thread")]
async fn owned_file_streams_and_reports_progress() {
    let dir = tmpdir("owned");
    let a_ep = endpoint(vec![iroh_blobs::ALPN.to_vec()]).await;
    let b_ep = endpoint(vec![]).await;
    let a = hub_open(&dir.join("a"), true).await.unwrap();
    let b = hub_open(&dir.join("b"), false).await.unwrap();
    let proto = a.protocol();
    let ep = a_ep.clone();
    tokio::spawn(async move {
        while let Some(inc) = ep.accept().await {
            let proto = proto.clone();
            tokio::spawn(async move {
                if let Ok(conn) = inc.await {
                    use iroh::protocol::ProtocolHandler;
                    let _ = proto.accept(conn).await;
                }
            });
        }
    });
    // 1 MiB + 1 byte: 17 cipher chunks, the last one 1 byte.
    let plain: Vec<u8> = (0..(1 << 20) + 1u32).map(|i| (i.wrapping_mul(2654435761) >> 7) as u8).collect();
    let src = dir.join("plain.bin");
    std::fs::write(&src, &plain).unwrap();
    let key = crypt::new_key();
    let mut seen = 0u64;
    let (hash, size) = a.add_file_owned("group/g", key, src, move |n| seen = seen.max(n)).await.unwrap();
    assert_eq!(size, plain.len() as u64);
    let total = crypt::cipher_len(size);
    assert_eq!(a.complete_size(&hash).await, Some(total));
    // Only the owner's tag protects it: releasing it lets it go (the shared tag was never set).
    assert!(a.store.tags().get(tag_of(&hash)).await.unwrap().is_none());
    assert!(a.store.tags().get(BlobHub::owned_tag("group/g", &hash)).await.unwrap().is_some());

    let conn = b_ep.connect(a_ep.addr(), iroh_blobs::ALPN).await.unwrap();
    let steps = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let s2 = steps.clone();
    b.fetch_owned_progress("group/g", conn, hash, total, move |n, t| s2.lock().push((n, t))).await.unwrap();
    let steps = steps.lock().clone();
    assert!(steps.len() > 2, "progress came in steps: {steps:?}");
    assert_eq!(steps.last(), Some(&(total, total)));
    let out = dir.join("out.bin");
    b.export_plain(hash, key, out.clone(), |_| {}).await.unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), plain);
    let _ = std::fs::remove_dir_all(&dir);
}

// KIND: unit (group files, verified streaming) — a provider that alters one byte deep inside the
// blob is caught at that chunk: the fetch fails with `Corrupt` ("does not match the signed
// hash"), the blob is never complete, and only the verified bytes before the bad chunk are kept.
#[tokio::test(flavor = "multi_thread")]
async fn altered_chunk_is_rejected_at_that_chunk() {
    let dir = tmpdir("tamper");
    let honest = hub_open(&dir.join("honest"), true).await.unwrap();
    let b = hub_open(&dir.join("b"), false).await.unwrap();
    let plain: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
    let src = dir.join("plain.bin");
    std::fs::write(&src, &plain).unwrap();
    let (hash, size) = honest.add_file_owned("group/g", crypt::new_key(), src, |_| {}).await.unwrap();
    let cipher = honest.read_cipher(&hash).await.unwrap();
    let total = crypt::cipher_len(size);
    assert_eq!(cipher.len() as u64, total);
    let mut bad = cipher.clone();
    let at = 400_000;
    bad[at] ^= 0x01;

    let evil_ep = endpoint(vec![iroh_blobs::ALPN.to_vec()]).await;
    tampering_provider(evil_ep.clone(), bad, cipher);
    let b_ep = endpoint(vec![]).await;
    let conn = b_ep.connect(evil_ep.addr(), iroh_blobs::ALPN).await.unwrap();
    let r = b.fetch_owned("group/g", conn, hash, total).await;
    match &r {
        Err(Error::Corrupt(m)) => assert!(m.contains("hash mismatch"), "{m}"),
        other => panic!("expected Corrupt, got {other:?}"),
    }
    assert!(r.unwrap_err().to_string().starts_with("data does not match the signed hash"));
    assert_eq!(b.complete_size(&hash).await, None, "never complete");
    let kept = b.local_bytes(&hash).await;
    assert!(kept <= at as u64, "nothing at or after the altered byte is kept: {kept}");
    let _ = std::fs::remove_dir_all(&dir);
}
