//! Blob storage and transfer: iroh-blobs on the node's own endpoint.
//!
//! Everything in the store is ciphertext (see `crypt.rs`); the hash is the ciphertext's BLAKE3.
//! Each hash we hold carries a tag `tl/<hex>` (what protects it from the store's GC); dropping
//! the tag lets the GC reclaim the bytes within `GC_INTERVAL`.
//!
//! Serving is gated: the store's event hook asks `Gate` before every get, for the connection's
//! authenticated device key and the hash asked for. `push` and `observe` are always refused.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use iroh_blobs::api::blobs::BlobStatus;
use iroh_blobs::provider::events::{AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage, RequestMode, ThrottleMode};
use iroh_blobs::store::GcConfig;
use iroh_blobs::store::fs::{FsStore, options::Options};
use iroh_blobs::{BlobsProtocol, Hash, HashAndFormat};
use n0_future::StreamExt;
use parking_lot::Mutex;

use super::crypt;
use crate::Error;

pub const GC_INTERVAL: Duration = Duration::from_secs(30);

/// A fetch that makes no progress for this long is given up (the connection may be alive but
/// the peer has stopped sending, e.g. its app was suspended mid-transfer).
pub const STALL: Duration = Duration::from_secs(20);

/// Whether `device` (an authenticated endpoint key) may read the blob `hash`.
pub type Gate = Arc<dyn Fn(&[u8; 32], &Hash) -> bool + Send + Sync>;

/// Live upload progress: `(device, hash, bytes sent so far, total)`.
pub type UploadProgress = Arc<dyn Fn(&[u8; 32], &Hash, u64, u64) + Send + Sync>;

/// A transfer we were serving ended: `(device, hash, completed)`. `completed == false` means
/// the provider reported it aborted (the peer went away, reset the stream, or we failed).
pub type UploadEnd = Arc<dyn Fn(&[u8; 32], &Hash, bool) + Send + Sync>;

/// Test-only knobs for the serving side, read once at `open` (like `P2P_CHAT_CLOCK_MS_OFFSET`):
/// `P2P_BLOB_TEST_THROTTLE_MS` delays every 16 KiB chunk we send by that many ms;
/// `P2P_BLOB_TEST_STALL_FILE` holds all sending (connection stays up) while that file exists.
#[derive(Clone, Default)]
pub(crate) struct TestKnobs {
    pub(crate) delay_ms: u64,
    pub(crate) stall_file: Option<PathBuf>,
}

impl TestKnobs {
    fn from_env() -> Self {
        Self {
            delay_ms: std::env::var("P2P_BLOB_TEST_THROTTLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
            stall_file: std::env::var_os("P2P_BLOB_TEST_STALL_FILE").map(PathBuf::from),
        }
    }
    fn active(&self) -> bool {
        self.delay_ms > 0 || self.stall_file.is_some()
    }
}

pub struct BlobHub {
    store: FsStore,
    dir: PathBuf,
    proto: BlobsProtocol,
}

fn io_err(e: impl std::fmt::Display) -> Error {
    Error::Io(e.to_string())
}

pub fn tag_of(hash: &Hash) -> String {
    format!("tl/{hash}")
}

impl BlobHub {
    /// Opens (or creates) the blob store under `dir` and starts the serving gate.
    pub async fn open(dir: &Path, gate: Gate, progress: UploadProgress, ended: UploadEnd) -> Result<Self, Error> {
        Self::open_with(dir, gate, progress, ended, TestKnobs::from_env()).await
    }

    pub(crate) async fn open_with(dir: &Path, gate: Gate, progress: UploadProgress, ended: UploadEnd, knobs: TestKnobs) -> Result<Self, Error> {
        std::fs::create_dir_all(dir.join("tmp"))?;
        // Leftovers of an interrupted import or export.
        for e in std::fs::read_dir(dir.join("tmp"))?.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
        let mut opts = Options::new(dir);
        opts.gc = Some(GcConfig { interval: GC_INTERVAL, add_protected: None });
        let store = FsStore::load_with_opts(dir.join("blobs.db"), opts).await.map_err(io_err)?;
        let mask = EventMask {
            connected: ConnectMode::Intercept,
            get: RequestMode::InterceptLog,
            get_many: RequestMode::InterceptLog,
            push: RequestMode::Disabled,
            observe: ObserveMode::Intercept,
            throttle: if knobs.active() { ThrottleMode::Intercept } else { ThrottleMode::None },
        };
        let (events, rx) = EventSender::channel(64, mask);
        tokio::spawn(gate_loop(rx, gate, progress, ended, knobs));
        let proto = BlobsProtocol::new(&store, Some(events));
        Ok(Self { store, dir: dir.to_path_buf(), proto })
    }

    pub fn protocol(&self) -> BlobsProtocol {
        self.proto.clone()
    }

    fn tmp(&self, what: &str) -> PathBuf {
        use rand::RngCore;
        let mut b = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut b);
        self.dir.join("tmp").join(format!("{what}-{}", hex::encode(b)))
    }

    /// Encrypts the file at `src` under `key` and stores the ciphertext. Returns the
    /// ciphertext hash and the plaintext size. `progress` gets plaintext bytes encrypted.
    pub async fn add_file(
        &self,
        key: [u8; 32],
        src: PathBuf,
        progress: impl FnMut(u64) + Send + 'static,
    ) -> Result<(Hash, u64), Error> {
        let tmp = self.tmp("enc");
        let out = tmp.clone();
        let size = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            let r = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&src)?);
            let w = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&out)?);
            crypt::encrypt_stream(&key, r, w, progress)
        })
        .await
        .map_err(io_err)?;
        let size = match size {
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(Error::Io(e.to_string()));
            }
        };
        let added = self.import_tmp(&tmp).await;
        let _ = std::fs::remove_file(&tmp);
        Ok((added?, size))
    }

    /// Encrypts `data` under `key` and stores the ciphertext.
    pub async fn add_bytes(&self, key: [u8; 32], data: Vec<u8>) -> Result<Hash, Error> {
        let ct = tokio::task::spawn_blocking(move || crypt::encrypt_bytes(&key, &data)).await.map_err(io_err)?;
        let tag = self.store.add_bytes(ct.clone()).temp_tag().await.map_err(io_err)?;
        let hash = tag.hash();
        self.keep(&hash).await?;
        drop(tag);
        Ok(hash)
    }

    async fn import_tmp(&self, path: &Path) -> Result<Hash, Error> {
        let tag = self.store.add_path(path).temp_tag().await.map_err(io_err)?;
        let hash = tag.hash();
        self.keep(&hash).await?;
        drop(tag);
        Ok(hash)
    }

    /// Tags `hash` so the GC leaves it (and any partial download of it) alone.
    pub async fn keep(&self, hash: &Hash) -> Result<(), Error> {
        self.store.tags().set(tag_of(hash), HashAndFormat::raw(*hash)).await.map_err(io_err)
    }

    /// Drops our claim on `hash`; the GC frees the bytes soon.
    pub async fn release(&self, hash: &Hash) -> Result<(), Error> {
        self.store.tags().delete(tag_of(hash)).await.map_err(io_err)?;
        Ok(())
    }

    /// Size of the complete blob, if we hold all of it.
    pub async fn complete_size(&self, hash: &Hash) -> Option<u64> {
        match self.store.blobs().status(*hash).await {
            Ok(BlobStatus::Complete { size }) => Some(size),
            _ => None,
        }
    }

    /// Ciphertext bytes of `hash` already verified and stored here (a partial download counts).
    pub async fn local_bytes(&self, hash: &Hash) -> u64 {
        match self.store.remote().local(HashAndFormat::raw(*hash)).await {
            Ok(info) => info.local_bytes(),
            Err(_) => 0,
        }
    }

    /// Fetches the ciphertext of `hash` over `conn` (an iroh-blobs connection to a peer that
    /// holds it), resuming what is already here. `cipher_total` is the expected ciphertext
    /// length; `progress` gets `(ciphertext bytes so far, total)`. Gives up with
    /// `Error::Timeout` when nothing arrives for `STALL`.
    pub async fn fetch(
        &self,
        conn: Connection,
        hash: Hash,
        cipher_total: u64,
        progress: impl FnMut(u64, u64) + Send,
    ) -> Result<(), Error> {
        self.fetch_with_stall(conn, hash, cipher_total, STALL, progress).await
    }

    pub async fn fetch_with_stall(
        &self,
        conn: Connection,
        hash: Hash,
        cipher_total: u64,
        stall: Duration,
        mut progress: impl FnMut(u64, u64) + Send,
    ) -> Result<(), Error> {
        use iroh_blobs::api::remote::GetProgressItem;
        self.keep(&hash).await?;
        if self.complete_size(&hash).await.is_some() {
            return Ok(());
        }
        let mut stream = Box::pin(self.store.remote().fetch(conn.clone(), HashAndFormat::raw(hash)).stream());
        let res = loop {
            match tokio::time::timeout(stall, stream.next()).await {
                Err(_) => break Err(Error::Timeout),
                Ok(None) => break Ok(()),
                Ok(Some(GetProgressItem::Progress(n))) => progress(n.min(cipher_total), cipher_total),
                Ok(Some(GetProgressItem::Done(_))) => break Ok(()),
                Ok(Some(GetProgressItem::Error(e))) => break Err(Error::Net(e.to_string())),
            }
        };
        // The request future lives inside `stream`; dropping it drops the stream pair, and
        // closing the connection makes sure a peer that sits in a stalled send sees it end.
        drop(stream);
        if res.is_err() {
            conn.close(0u32.into(), b"fetch abandoned");
        }
        res?;
        match self.complete_size(&hash).await {
            Some(n) if n == cipher_total => {
                progress(cipher_total, cipher_total);
                Ok(())
            }
            Some(_) => Err(Error::Protocol("blob size does not match the message".into())),
            None => Err(Error::Net("transfer ended before the blob was complete".into())),
        }
    }

    /// The whole ciphertext, for small blobs (shard snapshots, voice).
    pub async fn read_cipher(&self, hash: &Hash) -> Result<Vec<u8>, Error> {
        let b = self.store.blobs().get_bytes(*hash).await.map_err(io_err)?;
        Ok(b.to_vec())
    }

    /// Decrypts the blob `hash` to `dest` (written to a temp file next to it, then renamed).
    pub async fn export_plain(
        &self,
        hash: Hash,
        key: [u8; 32],
        dest: PathBuf,
        progress: impl FnMut(u64) + Send + 'static,
    ) -> Result<u64, Error> {
        let cipher = self.tmp("exp");
        self.store.blobs().export(hash, &cipher).await.map_err(io_err)?;
        let (c2, d2) = (cipher.clone(), dest.clone());
        let res = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            let part = d2.with_extension("tinline-part");
            let r = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&c2)?);
            let w = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&part)?);
            match crypt::decrypt_stream(&key, r, w, progress) {
                Ok(n) => {
                    std::fs::rename(&part, &d2)?;
                    Ok(n)
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    Err(e)
                }
            }
        })
        .await
        .map_err(io_err)?;
        let _ = std::fs::remove_file(&cipher);
        res.map_err(|e| Error::Io(e.to_string()))
    }

    pub async fn shutdown(&self) {
        let _ = self.store.shutdown().await;
    }
}

/// Answers the store's permission questions and reports upload progress.
async fn gate_loop(
    mut rx: tokio::sync::mpsc::Receiver<ProviderMessage>,
    gate: Gate,
    progress: UploadProgress,
    ended: UploadEnd,
    knobs: TestKnobs,
) {
    // connection id -> authenticated device key
    let peers: Arc<Mutex<std::collections::HashMap<u64, [u8; 32]>>> = Default::default();
    while let Some(msg) = rx.recv().await {
        match msg {
            ProviderMessage::ClientConnected(m) => {
                let ok = match m.inner.endpoint_id {
                    Some(id) => {
                        peers.lock().insert(m.inner.connection_id, *id.as_bytes());
                        true
                    }
                    None => false,
                };
                let _ = m.tx.send(if ok { Ok(()) } else { Err(AbortReason::Permission) }).await;
            }
            ProviderMessage::ConnectionClosed(m) => {
                peers.lock().remove(&m.inner.connection_id);
            }
            ProviderMessage::GetRequestReceived(m) => {
                let device = peers.lock().get(&m.inner.connection_id).copied();
                let hash = m.inner.request.hash;
                let ok = device.is_some_and(|d| gate(&d, &hash));
                let _ = m.tx.send(if ok { Ok(()) } else { Err(AbortReason::Permission) }).await;
                if let (true, Some(device)) = (ok, device) {
                    let (progress, ended) = (progress.clone(), ended.clone());
                    let mut updates = m.rx;
                    tokio::spawn(async move {
                        let mut total = 0u64;
                        while let Ok(Some(u)) = updates.recv().await {
                            use iroh_blobs::provider::events::RequestUpdate;
                            match u {
                                RequestUpdate::Started(s) => total = s.size,
                                RequestUpdate::Progress(p) => progress(&device, &hash, p.end_offset, total),
                                RequestUpdate::Completed(_) => ended(&device, &hash, true),
                                RequestUpdate::Aborted(_) => ended(&device, &hash, false),
                            }
                        }
                    });
                }
            }
            ProviderMessage::GetManyRequestReceived(m) => {
                let device = peers.lock().get(&m.inner.connection_id).copied();
                let ok = device.is_some_and(|d| m.inner.request.hashes.iter().all(|h| gate(&d, h)));
                let _ = m.tx.send(if ok { Ok(()) } else { Err(AbortReason::Permission) }).await;
            }
            ProviderMessage::ObserveRequestReceived(m) => {
                let _ = m.tx.send(Err(AbortReason::Permission)).await;
            }
            ProviderMessage::PushRequestReceived(m) => {
                let _ = m.tx.send(Err(AbortReason::Permission)).await;
            }
            ProviderMessage::Throttle(m) => {
                let k = knobs.clone();
                tokio::spawn(async move {
                    if k.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(k.delay_ms)).await;
                    }
                    while k.stall_file.as_ref().is_some_and(|f| f.exists()) {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    let _ = m.tx.send(Ok(())).await;
                });
            }
            _ => {}
        }
    }
}
