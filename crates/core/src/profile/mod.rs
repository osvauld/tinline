//! Account profiles: existing own-device Loro sync + authenticated chat control + encrypted blobs.
pub(crate) mod model;
mod pictures;
mod test_api;
#[cfg(test)]
mod tests;
use crate::{
    Error,
    node::{Inner, Me, Node, Shared},
    store::{Backing, Disk},
    sync::{
        profilecache::{MAX_CACHE, ProfileCache},
        profiledoc::ProfileDoc,
    },
};
use model::{MAX_SNAPSHOT, picture, validate};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
pub(crate) const PROFILE_KEY: &str = "sync/profile";
pub(crate) const CACHE_KEY: &str = "sync/profile-cache";

#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountProfile {
    pub did: String,
    /// Extensible JSON object. Unknown fields are data, not markup or executable content.
    pub fields_json: String,
    pub picture_hash: Option<String>,
    pub picture_ready: bool,
}
#[uniffi::export(with_foreign)]
pub trait ProfileEvents: Send + Sync {
    fn on_profile_changed(&self, did: String);
}
fn sealed(s: &Shared) -> Result<storage::Sealed, Error> {
    match s.acct.as_ref().and_then(|a| a.backing.clone()) {
        Some(Backing::Sealed(db)) => Ok(db),
        _ => Err(Error::Protocol(
            "profile sync requires an unlocked account vault".into(),
        )),
    }
}
pub(crate) fn picture_marker(did: &str, hash: &str) -> String {
    format!(
        "profile/pictures/{}/{hash}",
        blake3::hash(did.as_bytes()).to_hex()
    )
}
fn project_availability(
    state: &mut crate::store::State,
    fields: &std::collections::BTreeMap<String, Value>,
) {
    if let Some(value) = fields.get("availability")
        && let Some(available) = value.get("available").and_then(Value::as_bool)
    {
        state.availability = crate::store::AvailabilityState {
            unavailable: !available,
            until: value.get("until").and_then(Value::as_u64),
        };
    }
}
fn check_write(i: &Inner) -> Result<(), Error> {
    if i.fail_writes
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
    {
        return Err(Error::Io("injected write failure".into()));
    }
    Ok(())
}
fn allowed(s: &Shared, did: &str) -> bool {
    s.state.contacts.iter().any(|c| c.did == did)
        && !s.state.blocked.contains(did)
        && !crate::block::blocked_me(&s.state, did)
}
fn metadata_ops(
    db: &storage::Sealed,
    did: &str,
    doc: &ProfileDoc,
) -> Result<Vec<storage::Op>, Error> {
    let mut ops = Vec::new();
    let current = picture(&doc.fields()?)?;
    if let Some(p) = &current {
        let marker = picture_marker(did, &p.hash);
        if db
            .get(&marker)
            .map_err(|e| Error::Io(e.to_string()))?
            .is_none()
        {
            ops.push(
                db.put_op(&marker, b"pending")
                    .map_err(|e| Error::Io(e.to_string()))?,
            );
        }
    }
    // Owner signatures are verified before reaching here. Namespaces prevent deleting another
    // owner's cache, even if they refer to the same encrypted blob.
    for hash in doc.deletions()?.keys() {
        // A concurrently selected picture wins over old-cache consent. Retain the permit
        // for later, rather than rejecting a valid merged document and losing convergence.
        if current.as_ref().is_some_and(|p| p.hash == *hash) {
            continue;
        }
        ops.push(storage::Op::Delete(picture_marker(did, hash)));
    }
    Ok(ops)
}

impl Inner {
    pub(crate) fn profile_blob_tags(
        &self,
        epoch: u64,
    ) -> Result<std::collections::HashSet<String>, Error> {
        let s = self.shared.lock();
        if s.epoch != epoch {
            return Err(Error::Locked);
        }
        let db = sealed(&s)?;
        Ok(db
            .list_prefixed("profile/pictures/")
            .map_err(|e| Error::Io(e.to_string()))?
            .into_iter()
            .map(|key| key.replacen("profile/pictures/", "profile/", 1))
            .collect())
    }

    pub(crate) fn profile_open(self: &Arc<Self>) -> Result<(), Error> {
        let mut s = self.shared.lock();
        let me = s.me.clone().ok_or(Error::Locked)?;
        if s.profile.is_some() {
            return Ok(());
        }
        let db = sealed(&s)?;
        let snap = db.get(PROFILE_KEY).map_err(|e| Error::Io(e.to_string()))?;
        let p = ProfileDoc::load(&me.device, snap.as_deref())?;
        let inherited = matches!(&s.disk, Some(Disk::V2(d)) if d.profile_inherited);
        let mut changed = false;
        if snap.is_none() && !inherited {
            changed = true;
            let name: String = me
                .profile
                .name
                .chars()
                .filter(|c| proto::safe_name_char(*c))
                .collect();
            p.set("name", json!(name))?;
            p.set("availability", json!({"available": !s.state.availability.unavailable, "until": s.state.availability.until}))?;
        }
        if matches!(&s.disk, Some(Disk::V2(d)) if d.profile_name_pending) {
            changed |= p.set("name", json!(me.profile.name))?;
        }
        validate(&p, me.id.did())?;
        let cache = ProfileCache::load(
            &me.device,
            db.get(CACHE_KEY)
                .map_err(|e| Error::Io(e.to_string()))?
                .as_deref(),
        )?;
        let cached = cache.profiles(&me.device)?;
        let mut state = s.state.clone();
        project_availability(&mut state, &p.fields()?);
        if changed {
            state.profile_outbox.extend(
                state
                    .contacts
                    .iter()
                    .filter(|c| allowed(&s, &c.did))
                    .map(|c| c.did.clone()),
            );
        }
        let mut ops = metadata_ops(&db, me.id.did(), &p)?;
        for (did, p) in cached {
            if allowed(&s, &did) {
                ops.extend(metadata_ops(&db, &did, &p)?);
            }
        }
        ops.push(
            db.put_op(PROFILE_KEY, &p.snapshot())
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        ops.push(
            db.put_op("state", &serde_json::to_vec(&state)?)
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        db.apply(&ops).map_err(|e| Error::Io(e.to_string()))?;
        s.state = state;
        s.profile = Some(Arc::new(p));
        s.profile_cache = Some(Arc::new(cache));
        Self::profile_project(&mut s)?;
        Ok(())
    }

    /// Projects the shared name into account summaries/tickets; the Loro doc remains authoritative.
    pub(crate) fn profile_project(s: &mut Shared) -> Result<(), Error> {
        let (Some(me), Some(p)) = (s.me.clone(), s.profile.as_ref()) else {
            return Ok(());
        };
        let fields = p.fields()?;
        project_availability(&mut s.state, &fields);
        if let Some(name) = fields.get("name").and_then(Value::as_str) {
            if name != me.profile.name {
                let mut profile = me.profile.clone();
                profile.name = name.into();
                s.me = Some(Arc::new(Me::load(profile, me.epoch)?));
                crate::node::retire_ticket(&mut s.state, true);
            }
            if let Some(Disk::V2(d)) = s.disk.as_mut() {
                let changed = d.name != name || d.profile_name_pending;
                d.name = name.into();
                d.profile_name_pending = false;
                if changed && s.committed {
                    // A crash here is repaired from the already-durable doc on next unlock.
                    if let (Some(a), Some(disk)) = (&s.acct, &s.disk) {
                        a.store.save_profile(disk)?;
                    }
                }
            }
        }
        if let Some(cache) = &s.profile_cache {
            for (did, doc) in cache.profiles(&me.device)? {
                if let Some(name) = doc.fields()?.get("name").and_then(Value::as_str)
                    && let Some(contact) = s.state.contacts.iter_mut().find(|c| c.did == did)
                {
                    contact.name = name.into();
                }
            }
        }
        Ok(())
    }

    fn profile_edit(
        self: &Arc<Self>,
        epoch: u64,
        edit: impl FnOnce(&ProfileDoc, &Me) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.profile_mutate(epoch, edit, None)
    }

    fn profile_mutate(
        self: &Arc<Self>,
        epoch: u64,
        edit: impl FnOnce(&ProfileDoc, &Me) -> Result<(), Error>,
        ready_hash: Option<&str>,
    ) -> Result<(), Error> {
        let did;
        {
            let mut s = self.shared.lock();
            let me = s.me.clone().ok_or(Error::Locked)?;
            if s.epoch != epoch {
                return Err(Error::Locked);
            }
            let live = s.profile.clone().ok_or(Error::Locked)?;
            let candidate = ProfileDoc::load(&me.device, Some(&live.snapshot()))?;
            edit(&candidate, &me)?;
            validate(&candidate, me.id.did())?;
            if candidate.vv() == live.vv() {
                return Ok(());
            }
            let db = sealed(&s)?;
            let mut state = s.state.clone();
            project_availability(&mut state, &candidate.fields()?);
            state.profile_outbox.extend(
                state
                    .contacts
                    .iter()
                    .filter(|c| allowed(&s, &c.did))
                    .map(|c| c.did.clone()),
            );
            let mut ops = metadata_ops(&db, me.id.did(), &candidate)?;
            if let Some(hash) = ready_hash {
                if !picture(&candidate.fields()?)?.is_some_and(|p| p.hash == hash) {
                    return Err(Error::Protocol("picture publication mismatch".into()));
                }
                ops.push(
                    db.put_op(&picture_marker(me.id.did(), hash), b"ready")
                        .map_err(|e| Error::Io(e.to_string()))?,
                );
            }
            ops.push(
                db.put_op(PROFILE_KEY, &candidate.snapshot())
                    .map_err(|e| Error::Io(e.to_string()))?,
            );
            ops.push(
                db.put_op("state", &serde_json::to_vec(&state)?)
                    .map_err(|e| Error::Io(e.to_string()))?,
            );
            check_write(self)?;
            db.apply(&ops).map_err(|e| Error::Io(e.to_string()))?;
            live.import(&candidate.snapshot())?;
            s.state = state;
            if let Err(e) = Self::profile_project(&mut s) {
                tracing::warn!("profile name projection: {e}");
            }
            did = me.id.did().to_string();
        }
        self.sync_bump();
        self.profile_notify(&did);
        self.profile_push();
        self.profile_apply_deletions(epoch, &did);
        Ok(())
    }

    pub(crate) fn profile_set_field(
        self: &Arc<Self>,
        key: String,
        value: Option<Value>,
    ) -> Result<(), Error> {
        let me = self.me()?;
        if key == "picture" {
            return Err(Error::Protocol("use the profile picture API".into()));
        }
        if value.is_none() && matches!(key.as_str(), "name" | "availability") {
            return Err(Error::Protocol("required profile field".into()));
        }
        self.profile_edit(me.epoch, |p, _| {
            match value {
                Some(v) => {
                    p.set(&key, v)?;
                }
                None => {
                    p.remove(&key)?;
                }
            }
            Ok(())
        })
    }

    pub(crate) fn profile_notify(&self, did: &str) {
        let ev = self.profile_events.lock().clone();
        if let Some(ev) = ev {
            ev.on_profile_changed(did.into());
        }
    }

    pub(crate) fn profile_for_new_contact(self: &Arc<Self>, did: &str) {
        let Ok(me) = self.me() else { return };
        {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch || !allowed(&s, did) {
                return;
            }
            s.state.profile_outbox.insert(did.into());
        }
        self.persist_in_background(me.epoch);
        self.chat_kick(did.into());
    }

    pub(crate) fn profile_push(self: &Arc<Self>) {
        let dids = {
            let s = self.shared.lock();
            s.state
                .profile_outbox
                .iter()
                .filter(|d| allowed(&s, d))
                .cloned()
                .collect::<Vec<_>>()
        };
        for did in dids {
            self.chat_profile_changed(&did);
        }
    }

    pub(crate) fn profile_pending(&self, did: &str) -> bool {
        let s = self.shared.lock();
        allowed(&s, did) && s.state.profile_outbox.contains(did)
    }

    pub(crate) fn profile_ack(
        &self,
        epoch: u64,
        did: &str,
        vv: &loro::VersionVector,
    ) -> Result<(), Error> {
        let mut s = self.shared.lock();
        if s.epoch != epoch {
            return Err(Error::Locked);
        }
        if let Some(p) = &s.profile
            && matches!(
                vv.partial_cmp(&p.vv()),
                Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
            )
        {
            let mut state = s.state.clone();
            state.profile_outbox.remove(did);
            sealed(&s)?
                .put("state", &serde_json::to_vec(&state)?)
                .map_err(|e| Error::Io(e.to_string()))?;
            s.state = state;
        }
        Ok(())
    }

    pub(crate) fn apply_profile_update(
        self: &Arc<Self>,
        epoch: u64,
        update: &[u8],
    ) -> Result<(), Error> {
        if update.len() > MAX_SNAPSHOT {
            return Err(Error::Protocol("profile update bounds".into()));
        }
        self.profile_edit(epoch, |candidate, _| {
            candidate.import(update)?;
            Ok(())
        })?;
        self.profile_download(self.me()?.id.did().into());
        Ok(())
    }

    /// Direct contact updates are accepted only inside the existing grant/attestation-checked session.
    pub(crate) fn profile_receive(
        self: &Arc<Self>,
        epoch: u64,
        did: &str,
        update: &[u8],
    ) -> Result<loro::VersionVector, Error> {
        let vv;
        {
            let mut s = self.shared.lock();
            let me = s.me.clone().ok_or(Error::Locked)?;
            if s.epoch != epoch || !allowed(&s, did) {
                return Err(Error::Rejected("profile not accepted".into()));
            }
            if update.len() > MAX_SNAPSHOT {
                return Err(Error::Protocol("profile update bounds".into()));
            }
            let cache = s.profile_cache.clone().ok_or(Error::Locked)?;
            let p = cache
                .profile(did, &me.device)?
                .unwrap_or_else(|| ProfileDoc::new(&me.device));
            p.import(update)?;
            validate(&p, did)?;
            vv = p.vv();
            let staged = ProfileCache::load(&me.device, Some(&cache.snapshot()))?;
            if staged.set(did, &p)? {
                if staged.snapshot().len() > MAX_CACHE {
                    return Err(Error::Protocol("profile cache bounds".into()));
                }
                let db = sealed(&s)?;
                let mut ops = metadata_ops(&db, did, &p)?;
                ops.push(
                    db.put_op(CACHE_KEY, &staged.snapshot())
                        .map_err(|e| Error::Io(e.to_string()))?,
                );
                check_write(self)?;
                db.apply(&ops).map_err(|e| Error::Io(e.to_string()))?;
                cache.import(&staged.snapshot())?;
                if let Some(name) = p.fields()?.get("name").and_then(Value::as_str)
                    && let Some(c) = s.state.contacts.iter_mut().find(|c| c.did == did)
                {
                    c.name = name.into();
                }
            }
        }
        self.sync_bump();
        self.events.on_contacts_changed();
        self.profile_notify(did);
        self.profile_apply_deletions(epoch, did);
        self.profile_download(did.into());
        Ok(vv)
    }

    /// Own devices vouch for cached contact snapshots. Merge inner Loro histories rather than
    /// letting an outer LWW cache field roll back a newer contact profile.
    pub(crate) fn apply_profile_cache_update(
        self: &Arc<Self>,
        epoch: u64,
        update: &[u8],
    ) -> Result<(), Error> {
        let dids;
        {
            let mut s = self.shared.lock();
            let me = s.me.clone().ok_or(Error::Locked)?;
            if s.epoch != epoch || update.len() > MAX_CACHE {
                return Err(Error::Protocol("profile cache epoch/bounds".into()));
            }
            let live = s.profile_cache.clone().ok_or(Error::Locked)?;
            let staged = ProfileCache::load(&me.device, Some(&live.snapshot()))?;
            if !staged.import(update)? {
                return Ok(());
            }
            let old = live.profiles(&me.device)?;
            let incoming = staged.profiles(&me.device)?;
            dids = incoming
                .keys()
                .filter(|d| allowed(&s, d))
                .cloned()
                .collect::<Vec<_>>();
            let db = sealed(&s)?;
            let mut ops = Vec::new();
            for (did, p) in incoming {
                if let Some(previous) = old.get(&did) {
                    p.import(&previous.snapshot())?;
                    validate(&p, &did)?;
                    staged.set(&did, &p)?;
                }
                if allowed(&s, &did) {
                    ops.extend(metadata_ops(&db, &did, &p)?);
                }
            }
            if staged.snapshot().len() > MAX_CACHE {
                return Err(Error::Protocol("profile cache bounds".into()));
            }
            ops.push(
                db.put_op(CACHE_KEY, &staged.snapshot())
                    .map_err(|e| Error::Io(e.to_string()))?,
            );
            check_write(self)?;
            db.apply(&ops).map_err(|e| Error::Io(e.to_string()))?;
            live.import(&staged.snapshot())?;
            if let Err(e) = Self::profile_project(&mut s) {
                tracing::warn!("profile projection: {e}");
            }
        }
        self.sync_bump();
        self.events.on_contacts_changed();
        for did in dids {
            self.profile_notify(&did);
            self.profile_apply_deletions(epoch, &did);
            self.profile_download(did);
        }
        Ok(())
    }

    pub(crate) fn profile_doc(&self, did: &str) -> Result<Option<ProfileDoc>, Error> {
        let s = self.shared.lock();
        let me = s.me.as_ref().ok_or(Error::Locked)?;
        if did == me.id.did() {
            return s
                .profile
                .as_ref()
                .map(|p| ProfileDoc::load(&me.device, Some(&p.snapshot())))
                .transpose();
        }
        if !s.state.contacts.iter().any(|c| c.did == did) {
            return Ok(None);
        }
        let cache = s.profile_cache.as_ref().ok_or(Error::Locked)?;
        cache.profile(did, &me.device)
    }
}

#[uniffi::export]
impl Node {
    pub fn set_profile_events(&self, events: Arc<dyn ProfileEvents>) {
        *self.inner.profile_events.lock() = Some(events);
    }
    pub fn account_profile(&self, did: String) -> Result<Option<AccountProfile>, Error> {
        let Some(doc) = self.inner.profile_doc(&did)? else {
            return Ok(None);
        };
        let mut fields = doc.fields()?;
        if !fields.contains_key("name") {
            let s = self.inner.shared.lock();
            let me = s.me.as_ref().ok_or(Error::Locked)?;
            let name = if did == me.id.did() {
                Some(me.profile.name.clone())
            } else {
                s.state
                    .contacts
                    .iter()
                    .find(|c| c.did == did)
                    .map(|c| c.name.clone())
            };
            if let Some(name) = name {
                fields.insert("name".into(), json!(name));
            }
        }
        let p = picture(&fields)?;
        let ready = if let Some(p) = &p {
            self.inner.profile_picture_ready(&did, &p.hash)?
        } else {
            false
        };
        Ok(Some(AccountProfile {
            did,
            fields_json: serde_json::to_string(&fields)?,
            picture_hash: p.map(|p| p.hash),
            picture_ready: ready,
        }))
    }
    pub fn set_profile_field(&self, key: String, json_value: Option<String>) -> Result<(), Error> {
        self.inner.me()?;
        if json_value.as_ref().is_some_and(|s| s.len() > 16 * 1024) {
            return Err(Error::Protocol("profile field too large".into()));
        }
        let v = json_value.map(|s| serde_json::from_str(&s)).transpose()?;
        self.inner.profile_set_field(key, v)
    }
    pub fn set_profile_picture(&self, bytes: Vec<u8>) -> Result<(), Error> {
        self.replace_profile_picture(bytes)
    }
    /// Replacing the owner's picture automatically requests deletion of the previous cached copy.
    pub fn replace_profile_picture(&self, bytes: Vec<u8>) -> Result<(), Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.profile_set_picture(bytes).await })?
    }
    pub fn clear_profile_picture(&self) -> Result<(), Error> {
        self.remove_profile_picture()
    }
    /// Removing the owner's picture automatically signs the previous-hash deletion request.
    pub fn remove_profile_picture(&self) -> Result<(), Error> {
        let me = self.inner.me()?;
        self.inner.profile_edit(me.epoch, |p, me| {
            let old = picture(&p.fields()?)?;
            p.remove("picture")?;
            if let Some(old) = old {
                p.request_deletion(
                    &old.hash,
                    &proto::profile::sign_picture_deletion(&me.id, &old.hash)?,
                )?;
            }
            Ok(())
        })
    }
    pub fn request_profile_picture_deletion(&self, hash: String) -> Result<(), Error> {
        let me = self.inner.me()?;
        self.inner.profile_edit(me.epoch, |p, me| {
            if picture(&p.fields()?)?.is_some_and(|pic| pic.hash == hash) {
                return Err(Error::Protocol(
                    "cannot request deletion of the current picture".into(),
                ));
            }
            p.request_deletion(
                &hash,
                &proto::profile::sign_picture_deletion(&me.id, &hash)?,
            )
        })
    }
    pub fn picture_deletion_permits(&self, did: String) -> Result<Vec<String>, Error> {
        let Some(doc) = self.inner.profile_doc(&did)? else {
            return Ok(Vec::new());
        };
        doc.deletions()?
            .into_values()
            .map(|p| serde_json::to_string(&p).map_err(Error::from))
            .collect()
    }
    pub fn profile_picture(&self, did: String) -> Result<Option<Vec<u8>>, Error> {
        let Some(doc) = self.inner.profile_doc(&did)? else {
            return Ok(None);
        };
        let Some(p) = picture(&doc.fields()?)? else {
            return Ok(None);
        };
        let inner = self.inner.clone();
        self.block_on(async move { inner.profile_picture_local(&did, &p).await })?
    }
}
