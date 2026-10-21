//! Contact profile snapshots vouched for by our own authenticated devices, like own chat history.
//! This cache is never exported to contacts. Only the account's `profile` doc is public to them.
use super::{
    doc::{Doc, entries, io, text},
    profiledoc::ProfileDoc,
};
use crate::{
    Error,
    profile::model::{MAX_SNAPSHOT, validate},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;
use std::ops::Deref;
pub const PROFILE_CACHE_DOC: &str = "profile-cache";
pub const MAX_CACHE: usize = 12 * 1024 * 1024;
pub struct ProfileCache(Doc);
impl Deref for ProfileCache {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.0
    }
}
impl ProfileCache {
    pub fn new(device: &[u8; 32]) -> Self {
        Self(Doc::new(device, PROFILE_CACHE_DOC))
    }
    pub fn load(device: &[u8; 32], snapshot: Option<&[u8]>) -> Result<Self, Error> {
        if snapshot.is_none() {
            return Ok(Self::new(device));
        }
        Ok(Self(Doc::load(device, PROFILE_CACHE_DOC, snapshot)?))
    }
    pub fn profile(&self, did: &str, device: &[u8; 32]) -> Result<Option<ProfileDoc>, Error> {
        let Some(value) = self.doc.get_map("profiles").get(did) else {
            return Ok(None);
        };
        let loro::ValueOrContainer::Value(value) = value else {
            return Err(Error::Protocol("invalid cached profile".into()));
        };
        Self::decode(did, &value, device).map(Some)
    }
    fn decode(did: &str, value: &loro::LoroValue, device: &[u8; 32]) -> Result<ProfileDoc, Error> {
        if identity::public_key_from_did(did).is_none() {
            return Err(Error::Protocol("invalid cached profile DID".into()));
        }
        let encoded =
            text(value).ok_or_else(|| Error::Protocol("invalid cached profile".into()))?;
        if encoded.len() > MAX_SNAPSHOT * 2 {
            return Err(Error::Protocol("cached profile bounds".into()));
        }
        let bytes = STANDARD.decode(encoded).map_err(io)?;
        if bytes.len() > MAX_SNAPSHOT {
            return Err(Error::Protocol("cached profile bounds".into()));
        }
        let p = ProfileDoc::new(device);
        p.import(&bytes)?;
        validate(&p, did)?;
        Ok(p)
    }
    pub fn profiles(&self, device: &[u8; 32]) -> Result<BTreeMap<String, ProfileDoc>, Error> {
        let root = self.doc.get_deep_value();
        if entries(&root)
            .iter()
            .any(|(key, v)| key != "profiles" || !matches!(v, loro::LoroValue::Map(_)))
        {
            return Err(Error::Protocol("invalid profile cache root".into()));
        }
        let values = self.doc.get_map("profiles").get_deep_value();
        let all = entries(&values);
        if all.len() > 1024 {
            return Err(Error::Protocol("too many cached profiles".into()));
        }
        all.into_iter()
            .map(|(did, v)| {
                let p = Self::decode(&did, v, device)?;
                Ok((did, p))
            })
            .collect()
    }
    pub fn set(&self, did: &str, p: &ProfileDoc) -> Result<bool, Error> {
        let value = STANDARD.encode(p.snapshot());
        let m = self.doc.get_map("profiles");
        if m.get(did)
            .and_then(|v| match v {
                loro::ValueOrContainer::Value(v) => text(&v),
                _ => None,
            })
            .as_deref()
            == Some(&value)
        {
            return Ok(false);
        }
        m.insert(did, value).map_err(io)?;
        self.doc.commit();
        Ok(true)
    }
}
