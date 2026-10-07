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

/// Whether `device` (an authenticated endpoint key) may read the blob `hash`.
pub type Gate = Arc<dyn Fn(&[u8; 32], &Hash) -> bool + Send + Sync>;

/// Live upload progress: `(device, hash, bytes sent so far, total)`.
pub type UploadProgress = Arc<dyn Fn(&[u8; 32], &Hash, u64, u64) + Send + Sync>;

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
    pub async fn open(dir: &Path, gate: Gate, progress: UploadProgress) -> Result<Self, Error> {
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
            throttle: ThrottleMode::None,
        };
        let (events, rx) = EventSender::channel(64, mask);
        tokio::spawn(gate_loop(rx, gate, progress));
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

    /// Fetches the ciphertext of `hash` over `conn` (an iroh-blobs connection to a peer that
    /// holds it), resuming what is already here. `progress` gets `(bytes so far, total)`.
    pub async fn fetch(
        &self,
        conn: Connection,
        hash: Hash,
        total: u64,
        mut progress: impl FnMut(u64, u64) + Send,
    ) -> Result<(), Error> {
        self.keep(&hash).await?;
        if self.complete_size(&hash).await.is_some() {
            return Ok(());
        }
        let mut stream = self.store.remote().fetch(conn, HashAndFormat::raw(hash)).stream();
        while let Some(item) = stream.next().await {
            match item {
                iroh_blobs::api::remote::GetProgressItem::Progress(n) => progress(n.min(total), total),
                iroh_blobs::api::remote::GetProgressItem::Done(_) => break,
                iroh_blobs::api::remote::GetProgressItem::Error(e) => return Err(Error::Net(e.to_string())),
            }
        }
        match self.complete_size(&hash).await {
            Some(n) if n == crypt::cipher_len(total) || total == 0 => {
                progress(total, total);
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
async fn gate_loop(mut rx: tokio::sync::mpsc::Receiver<ProviderMessage>, gate: Gate, progress: UploadProgress) {
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
                    let progress = progress.clone();
                    let mut updates = m.rx;
                    tokio::spawn(async move {
                        let mut total = 0u64;
                        while let Ok(Some(u)) = updates.recv().await {
                            use iroh_blobs::provider::events::RequestUpdate;
                            match u {
                                RequestUpdate::Started(s) => total = s.size,
                                RequestUpdate::Progress(p) => progress(&device, &hash, p.end_offset, total),
                                _ => {}
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
                let _ = m.tx.send(Ok(())).await;
            }
            _ => {}
        }
    }
}
