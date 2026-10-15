//! Profile field validation only. Loro and the existing authenticated channels handle sync.
use crate::{Error, sync::profiledoc::ProfileDoc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const MAX_SNAPSHOT: usize = 1024 * 1024;
pub const MAX_PICTURE: usize = 1024 * 1024;
fn bad(what: &str) -> Error {
    Error::Protocol(format!("profile: {what}"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    pub hash: String,
    pub key: String,
    pub content_hash: String,
    pub size: u64,
}
impl Picture {
    pub fn validate(&self) -> Result<(), Error> {
        for h in [&self.hash, &self.key, &self.content_hash] {
            if h.len() != 64
                || !h
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(bad("bad picture hash/key"));
            }
        }
        if self.size == 0 || self.size > (MAX_PICTURE + 4096) as u64 {
            return Err(bad("picture size"));
        }
        Ok(())
    }
}
pub fn picture(fields: &BTreeMap<String, Value>) -> Result<Option<Picture>, Error> {
    fields
        .get("picture")
        .map(|v| {
            let p: Picture =
                serde_json::from_value(v.clone()).map_err(|_| bad("picture descriptor"))?;
            p.validate()?;
            Ok(p)
        })
        .transpose()
}
pub fn validate(doc: &ProfileDoc, owner: &str) -> Result<(), Error> {
    doc.validate_root()?;
    let fields = doc.fields()?;
    if fields.len() > 64 || serde_json::to_vec(&fields)?.len() > 64 * 1024 {
        return Err(bad("fields too large"));
    }
    for (k, v) in &fields {
        if k.is_empty()
            || k.len() > 256
            || k.chars().any(char::is_control)
            || serde_json::to_vec(v)?.len() > 16 * 1024
        {
            return Err(bad("field bounds"));
        }
        match k.as_str() {
            "name"
                if v.as_str().is_none_or(|s| {
                    s.len() > 256 || s.chars().any(|c| !proto::safe_name_char(c))
                }) =>
            {
                return Err(bad("name"));
            }
            "status" | "about" if v.as_str().is_none_or(|s| s.len() > 4096) => {
                return Err(bad("status/about"));
            }
            "availability" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Availability {
                    available: bool,
                    until: Option<u64>,
                }
                let a: Availability =
                    serde_json::from_value(v.clone()).map_err(|_| bad("availability"))?;
                if a.available && a.until.is_some() {
                    return Err(bad("available with pause deadline"));
                }
            }
            _ => {}
        }
    }
    picture(&fields)?;
    for (hash, permit) in doc.deletions()? {
        let p = proto::profile::verify_picture_deletion(&permit, owner)?;
        if p.claim.picture_hash != hash {
            return Err(bad("invalid picture deletion target"));
        }
    }
    if doc.snapshot().len() > MAX_SNAPSHOT {
        return Err(bad("snapshot bounds"));
    }
    Ok(())
}
