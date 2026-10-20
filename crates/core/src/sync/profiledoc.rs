//! Account-scoped profile data in its own Loro document.
//!
//! Fields are independently merged JSON values. Unknown fields are preserved; callers patch
//! only the fields they intend to change. This is the document primitive, not a network trust
//! boundary: owner authentication, bounded schema validation and picture transfer belong to
//! the integration layer before accepting remote updates.

use std::collections::BTreeMap;
use std::ops::Deref;

use serde_json::Value;

use super::doc::{Doc, entries, io, text};
use crate::Error;

pub const PROFILE_DOC: &str = "profile";

pub struct ProfileDoc(Doc);

impl Deref for ProfileDoc {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.0
    }
}

impl ProfileDoc {
    pub fn new(device: &[u8; 32]) -> Self {
        Self(Doc::new(device, PROFILE_DOC))
    }

    pub fn load(device: &[u8; 32], snapshot: Option<&[u8]>) -> Result<Self, Error> {
        Ok(Self(Doc::load(device, PROFILE_DOC, snapshot)?))
    }

    /// Reads all fields, including extensions unknown to this client. Invalid representations
    /// produce an error rather than silently disappearing from a subsequently saved profile.
    pub fn fields(&self) -> Result<BTreeMap<String, Value>, Error> {
        let root = self.doc.get_map("fields").get_deep_value();
        entries(&root)
            .into_iter()
            .map(|(key, value)| {
                let json = text(value)
                    .ok_or_else(|| Error::Protocol("profile field is not JSON text".into()))?;
                let value = serde_json::from_str(&json).map_err(io)?;
                Ok((key, value))
            })
            .collect()
    }

    pub fn validate_root(&self) -> Result<(), Error> {
        let root = self.doc.get_deep_value();
        for (key, value) in entries(&root) {
            if !matches!(key.as_str(), "fields" | "deletions")
                || !matches!(value, loro::LoroValue::Map(_))
            {
                return Err(Error::Protocol("invalid profile root".into()));
            }
        }
        Ok(())
    }

    pub fn deletions(
        &self,
    ) -> Result<BTreeMap<String, proto::profile::SignedPictureDeletion>, Error> {
        let root = self.doc.get_map("deletions").get_deep_value();
        let values = entries(&root);
        if values.len() > 4096 {
            return Err(Error::Protocol("too many picture deletion permits".into()));
        }
        values
            .into_iter()
            .map(|(hash, value)| {
                let json = text(value)
                    .ok_or_else(|| Error::Protocol("invalid picture deletion permit".into()))?;
                if json.len() > 4096 {
                    return Err(Error::Protocol("picture deletion permit too large".into()));
                }
                Ok((hash, serde_json::from_str(&json)?))
            })
            .collect()
    }

    pub fn request_deletion(
        &self,
        hash: &str,
        permit: &proto::profile::SignedPictureDeletion,
    ) -> Result<(), Error> {
        if self.deletions()?.get(hash) == Some(permit) {
            return Ok(());
        }
        self.doc
            .get_map("deletions")
            .insert(hash, serde_json::to_string(permit)?)
            .map_err(io)?;
        self.doc.commit();
        Ok(())
    }

    /// Patches one field without rewriting any other fields. JSON null is a stored value;
    /// use `remove` for an actual deletion.
    pub fn set(&self, key: &str, value: Value) -> Result<bool, Error> {
        if self.fields()?.get(key) == Some(&value) {
            return Ok(false);
        }
        let json = serde_json::to_string(&value).map_err(io)?;
        self.doc.get_map("fields").insert(key, json).map_err(io)?;
        self.doc.commit();
        Ok(true)
    }

    /// Explicit deletion syncs to other devices. Missing keys are a no-op.
    pub fn remove(&self, key: &str) -> Result<bool, Error> {
        if !self.fields()?.contains_key(key) {
            return Ok(false);
        }
        self.doc.get_map("fields").delete(key).map_err(io)?;
        self.doc.commit();
        Ok(true)
    }
}
