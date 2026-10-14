//! Picture normalization and encrypted transfer reuse the chat BlobHub and its serving gate.
use super::{
    Error, Inner,
    model::{MAX_PICTURE, Picture, picture},
    picture_marker, sealed,
};
use crate::{chat::crypt, node::addr_for};
use image::{ImageFormat, ImageReader};
use iroh_blobs::Hash;
use rand::RngCore;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

fn bad(e: impl std::fmt::Display) -> Error {
    Error::Protocol(format!("profile picture: {e}"))
}
fn normalize(bytes: &[u8], received: bool) -> Result<Vec<u8>, Error> {
    if bytes.is_empty()
        || bytes.len()
            > if received {
                MAX_PICTURE
            } else {
                2 * MAX_PICTURE
            }
    {
        return Err(bad("size"));
    }
    let mut r = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(bad)?;
    if !matches!(
        r.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) || (received && r.format() != Some(ImageFormat::Png))
    {
        return Err(bad("format"));
    }
    let max = if received { 512 } else { 4096 };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max);
    limits.max_image_height = Some(max);
    limits.max_alloc = Some(64 * 1024 * 1024);
    r.limits(limits);
    let img = r.decode().map_err(bad)?;
    if img.width() == 0 || img.height() == 0 {
        return Err(bad("dimensions"));
    }
    if received {
        return Ok(bytes.to_vec());
    }
    let mut out = Cursor::new(Vec::new());
    img.thumbnail(512, 512)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(bad)?;
    let out = out.into_inner();
    if out.len() > MAX_PICTURE {
        return Err(bad("normalized size"));
    }
    Ok(out)
}
fn decrypt(p: &Picture, cipher: &[u8]) -> Result<Vec<u8>, Error> {
    if cipher.len() as u64 != p.size || blake3::hash(cipher).to_hex().as_str() != p.hash {
        return Err(bad("ciphertext hash/size"));
    }
    let key: [u8; 32] = hex::decode(&p.key)
        .map_err(bad)?
        .try_into()
        .map_err(|_| bad("key"))?;
    let plain = crypt::decrypt_bytes(&key, cipher).map_err(bad)?;
    if blake3::hash(&plain).to_hex().as_str() != p.content_hash {
        return Err(bad("content hash"));
    }
    normalize(&plain, true)
}
impl Inner {
    pub(crate) async fn profile_set_picture(self: &Arc<Self>, bytes: Vec<u8>) -> Result<(), Error> {
        let me = self.me()?;
        let plain = tokio::task::spawn_blocking(move || normalize(&bytes, false))
            .await
            .map_err(bad)??;
        let content_hash = blake3::hash(&plain).to_hex().to_string();
        if self.profile_doc(me.id.did())?.is_some_and(|d| {
            picture(&d.fields().unwrap_or_default())
                .ok()
                .flatten()
                .is_some_and(|p| p.content_hash == content_hash)
        }) {
            return Ok(());
        }
        let mut key = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        let hub = self.chat_hub().await?;
        if self.me()?.epoch != me.epoch {
            return Err(Error::Locked);
        }
        let hash = hub.add_profile_bytes(me.id.did(), key, plain).await?;
        let _claim = hub.profile_claim(me.id.did(), &hash).await;
        hub.keep_profile(me.id.did(), &hash).await?;
        let size = hub
            .complete_size(&hash)
            .await
            .ok_or_else(|| bad("incomplete local blob"))?;
        let descriptor = Picture {
            hash: hash.to_string(),
            key: hex::encode(key),
            content_hash,
            size,
        };
        // Persist verified bytes first, then publish reference + readiness + old-hash request
        // in one account-store transaction. A failed publication leaves the old profile intact.
        let result = self.profile_mutate(
            me.epoch,
            |p, me| {
                let old = picture(&p.fields()?)?;
                p.set("picture", serde_json::to_value(&descriptor)?)?;
                if let Some(old) = old {
                    p.request_deletion(
                        &old.hash,
                        &proto::profile::sign_picture_deletion(&me.id, &old.hash)?,
                    )?;
                }
                Ok(())
            },
            Some(&descriptor.hash),
        );
        if result.is_err() {
            let _ = hub.release_profile(me.id.did(), &hash).await;
        }
        result
    }

    pub(crate) fn profile_picture_ready(&self, did: &str, hash: &str) -> Result<bool, Error> {
        let s = self.shared.lock();
        s.me.as_ref().ok_or(Error::Locked)?;
        Ok(sealed(&s)?
            .get(&picture_marker(did, hash))
            .map_err(bad)?
            .as_deref()
            == Some(b"ready"))
    }
    pub(crate) async fn profile_picture_local(
        self: &Arc<Self>,
        did: &str,
        p: &Picture,
    ) -> Result<Option<Vec<u8>>, Error> {
        let epoch = self.me()?.epoch;
        if !self.profile_picture_ready(did, &p.hash)? {
            return Ok(None);
        }
        let hub = self.chat_hub().await?;
        let hash = p.hash.parse::<Hash>().map_err(bad)?;
        let _claim = hub.profile_claim(did, &hash).await;
        if hub.complete_size(&hash).await != Some(p.size) {
            return Ok(None);
        }
        let cipher = hub.read_cipher(&hash).await?;
        let bytes = decrypt(p, &cipher)?;
        if self.me()?.epoch != epoch {
            return Err(Error::Locked);
        }
        Ok(Some(bytes))
    }

    /// Called by the EXISTING BlobHub gate, never an alternate unauthenticated serving path.
    pub(crate) fn profile_may_serve(&self, device: &[u8; 32], hash: &Hash) -> bool {
        let s = self.shared.lock();
        let Some(me) = &s.me else { return false };
        let own = s
            .state
            .registry
            .iter()
            .any(|e| e.device == *device && !e.removed && e.device != me.device);
        if !own
            && !s
                .state
                .contacts
                .iter()
                .any(|c| c.has_device(device) && super::allowed(&s, &c.did))
        {
            return false;
        }
        let selected = |p: &crate::sync::profiledoc::ProfileDoc| {
            picture(&p.fields().unwrap_or_default())
                .ok()
                .flatten()
                .is_some_and(|p| p.hash == hash.to_string())
        };
        if s.profile.as_deref().is_some_and(selected) {
            return true;
        }
        own && s
            .profile_cache
            .as_ref()
            .and_then(|c| c.profiles(&me.device).ok())
            .is_some_and(|v| v.values().any(selected))
    }

    pub(crate) fn profile_download(self: &Arc<Self>, did: String) {
        let Ok(me) = self.me() else { return };
        let Some(p) = self
            .profile_doc(&did)
            .ok()
            .flatten()
            .and_then(|d| picture(&d.fields().ok()?).ok().flatten())
        else {
            return;
        };
        if self.profile_picture_ready(&did, &p.hash).unwrap_or(false) {
            return;
        }
        let epoch = me.epoch;
        drop(me); // Background retries do not retain the account's mnemonic/signing identity.
        let token = (epoch, did.clone(), p.hash.clone());
        if !self.profile_downloads.lock().insert(token.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut delay = 1;
            loop {
                if this.me().is_err()
                    || this.shared.lock().epoch != epoch
                    || this.shared.lock().endpoint.is_none()
                {
                    break;
                }
                let Some(current) = this
                    .profile_doc(&did)
                    .ok()
                    .flatten()
                    .and_then(|d| picture(&d.fields().ok()?).ok().flatten())
                else {
                    break;
                };
                if current != p {
                    break;
                }
                let Ok(permit) = this.profile_fetches.clone().acquire_owned().await else {
                    break;
                };
                if this.shared.lock().epoch != epoch {
                    break;
                }
                let selected = this
                    .profile_doc(&did)
                    .ok()
                    .flatten()
                    .and_then(|d| picture(&d.fields().ok()?).ok().flatten());
                if selected.as_ref() != Some(&p) {
                    break;
                }
                let result = this.profile_fetch_picture(epoch, &did, &p).await;
                drop(permit);
                if result.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
            this.profile_downloads.lock().remove(&token);
        });
    }

    async fn profile_fetch_picture(
        self: &Arc<Self>,
        epoch: u64,
        did: &str,
        p: &Picture,
    ) -> Result<(), Error> {
        let me = self.me()?;
        if me.epoch != epoch {
            return Err(Error::Locked);
        }
        let ep = self.endpoint()?;
        let hub = self.chat_hub().await?;
        if self.me()?.epoch != epoch {
            return Err(Error::Locked);
        }
        let hash = p.hash.parse::<Hash>().map_err(bad)?;
        let claim = hub.profile_claim(did, &hash).await;
        if self.me()?.epoch != epoch {
            return Err(Error::Locked);
        }
        hub.keep_profile(did, &hash).await?;
        let mut sources = self
            .own_blob_sources()
            .into_iter()
            .map(|(d, r, _)| (d, r))
            .collect::<Vec<_>>();
        if did != me.id.did() {
            let s = self.shared.lock();
            if !super::allowed(&s, did) {
                return Err(Error::Rejected("picture not accepted".into()));
            }
            if let Some(c) = s.state.contacts.iter().find(|c| c.did == did) {
                sources.extend(c.devices.iter().map(|d| (d.device, d.relay.clone())));
            }
        }
        for (device, relay) in sources {
            if hub.complete_size(&hash).await == Some(p.size) {
                break;
            }
            let addr = addr_for(&device, relay.as_deref())?;
            let attempt = async {
                let conn = ep
                    .connect(addr, iroh_blobs::ALPN)
                    .await
                    .map_err(Error::net)?;
                let result = hub.fetch_profile(did, conn.clone(), hash, p.size).await;
                conn.close(0u32.into(), b"picture fetched");
                result
            };
            let _ = tokio::time::timeout(Duration::from_secs(30), attempt).await;
        }
        if hub.complete_size(&hash).await != Some(p.size) {
            return Err(bad("not downloaded"));
        }
        let bytes = hub.read_cipher(&hash).await?;
        decrypt(p, &bytes)?;
        let selected = self
            .profile_doc(did)?
            .and_then(|d| picture(&d.fields().ok()?).ok().flatten());
        let mut deleted = false;
        {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return Err(Error::Locked);
            }
            let db = sealed(&s)?;
            // A signed deletion arriving during transfer wins; never recreate its cache marker.
            if db
                .get(&picture_marker(did, &p.hash))
                .map_err(bad)?
                .is_none()
            {
                deleted = true;
            } else {
                db.put(&picture_marker(did, &p.hash), b"ready")
                    .map_err(bad)?;
            }
        }
        drop(claim);
        if deleted {
            self.profile_release_deleted(epoch, did, &p.hash).await?;
            // A concurrently selected reference can re-establish a pending marker while the
            // old transfer ends. Retry rather than losing its only scheduled download.
            return Err(bad("picture cache selection changed"));
        } else if selected.as_ref() == Some(p) {
            self.profile_notify(did);
        }
        Ok(())
    }

    pub(crate) fn profile_apply_deletions(self: &Arc<Self>, epoch: u64, did: &str) {
        let Some(doc) = self.profile_doc(did).ok().flatten() else {
            return;
        };
        let hashes = doc
            .deletions()
            .unwrap_or_default()
            .into_keys()
            .collect::<Vec<_>>();
        let this = self.clone();
        let did = did.to_string();
        self.handle.spawn(async move {
            for hash in hashes {
                let _ = this.profile_release_deleted(epoch, &did, &hash).await;
            }
        });
    }
    async fn profile_release_deleted(
        self: &Arc<Self>,
        epoch: u64,
        did: &str,
        hash: &str,
    ) -> Result<(), Error> {
        let hub = self.chat_hub().await?;
        let hash = hash.parse::<Hash>().map_err(bad)?;
        let _claim = hub.profile_claim(did, &hash).await;
        {
            let s = self.shared.lock();
            if s.epoch != epoch {
                return Err(Error::Locked);
            }
            if sealed(&s)?
                .get(&picture_marker(did, &hash.to_string()))
                .map_err(bad)?
                .is_some()
            {
                return Ok(());
            }
        }
        // Purpose- and owner-specific protection tags avoid races with other owners and chat.
        hub.release_profile(did, &hash).await
    }
}

#[cfg(test)]
mod tests;
