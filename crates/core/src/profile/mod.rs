//! Account profiles: the UniFFI surface. The engine (own-device Loro sync, authenticated chat
//! control, encrypted picture blobs) is `chat_runtime::profile`, re-exported here.
pub use chat_runtime::profile::*;
mod test_api;
use crate::{Error, node::Node};
use serde_json::json;
use model::picture;
use std::sync::Arc;

#[derive(Debug, Clone, uniffi::Record)]
pub struct AccountProfile {
    pub did: String,
    /// Extensible JSON object. Unknown fields are data, not markup or executable content.
    pub fields_json: String,
    pub picture_hash: Option<String>,
    pub picture_ready: bool,
}
#[uniffi::export(rust, foreign)]
pub trait ProfileEvents: Send + Sync {
    fn on_profile_changed(&self, did: String);
}
/// Hands the profile engine's events to the app's `ProfileEvents`.
pub(crate) struct ProfileEventsAdapter(pub(crate) Arc<dyn ProfileEvents>);

impl chat_runtime::ProfileSink for ProfileEventsAdapter {
    fn on_profile_changed(&self, did: String) {
        self.0.on_profile_changed(did);
    }
}

#[uniffi::export]
impl Node {
    pub fn set_profile_events(&self, events: Arc<dyn ProfileEvents>) {
        self.inner.chat.set_profile_sink(Arc::new(ProfileEventsAdapter(events)));
    }
    pub fn account_profile(&self, did: String) -> Result<Option<AccountProfile>, Error> {
        let Some(doc) = self.inner.chat.profile_doc(&did)? else {
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
            self.inner.chat.profile_picture_ready(&did, &p.hash)?
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
        Ok(self.inner.chat.profile_set_field(key, v)?)
    }
    pub fn set_profile_picture(&self, bytes: Vec<u8>) -> Result<(), Error> {
        self.replace_profile_picture(bytes)
    }
    /// Replacing the owner's picture automatically requests deletion of the previous cached copy.
    pub fn replace_profile_picture(&self, bytes: Vec<u8>) -> Result<(), Error> {
        let chat = self.inner.chat.clone();
        self.block_rt(async move { chat.profile_set_picture(bytes).await })
    }
    pub fn clear_profile_picture(&self) -> Result<(), Error> {
        self.remove_profile_picture()
    }
    /// Removing the owner's picture automatically signs the previous-hash deletion request.
    pub fn remove_profile_picture(&self) -> Result<(), Error> {
        let me = self.inner.me()?;
        Ok(self.inner.chat.profile_edit(me.epoch, |p, me| {
            let old = picture(&p.fields()?)?;
            p.remove("picture")?;
            if let Some(old) = old {
                p.request_deletion(
                    &old.hash,
                    &proto::profile::sign_picture_deletion(&me.id, &old.hash, crate::node::now_ms())?,
                )?;
            }
            Ok(())
        })?)
    }
    pub fn request_profile_picture_deletion(&self, hash: String) -> Result<(), Error> {
        let me = self.inner.me()?;
        Ok(self.inner.chat.profile_edit(me.epoch, |p, me| {
            if picture(&p.fields()?)?.is_some_and(|pic| pic.hash == hash) {
                return Err(node_runtime::Error::Protocol(
                    "cannot request deletion of the current picture".into(),
                ));
            }
            Ok(p.request_deletion(
                &hash,
                &proto::profile::sign_picture_deletion(&me.id, &hash, crate::node::now_ms())?,
            )?)
        })?)
    }
    pub fn picture_deletion_permits(&self, did: String) -> Result<Vec<String>, Error> {
        let Some(doc) = self.inner.chat.profile_doc(&did)? else {
            return Ok(Vec::new());
        };
        doc.deletions()?
            .into_values()
            .map(|p| serde_json::to_string(&p).map_err(Error::from))
            .collect()
    }
    pub fn profile_picture(&self, did: String) -> Result<Option<Vec<u8>>, Error> {
        let Some(doc) = self.inner.chat.profile_doc(&did)? else {
            return Ok(None);
        };
        let Some(p) = picture(&doc.fields()?)? else {
            return Ok(None);
        };
        let chat = self.inner.chat.clone();
        self.block_rt(async move { chat.profile_picture_local(&did, &p).await })
    }
}
