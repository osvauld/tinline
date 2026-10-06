//! What survives a restart, as JSON files in the app's private data dir. Every write goes to a
//! temp file then renames over the old one, so a kill mid-write leaves the previous state.
//!
//! The mnemonic and device secret sit here in the clear: the directory is the app sandbox,
//! and wrapping them in the Android Keystore is the platform side's job, not this crate's.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub mnemonic: String,
    pub name: String,
    /// The install's iroh secret key; random per install, attested by the DID.
    pub device_secret: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredContact {
    pub did: String,
    pub name: String,
    /// Newest first; calls dial the first.
    pub devices: Vec<[u8; 32]>,
    pub relay: Option<String>,
    /// Lets us call them; replaced whenever they renew it.
    pub grant_from_them: proto::SignedGrant,
    pub added_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub contacts: Vec<StoredContact>,
    /// Ids of grants we issued and then took back.
    pub revoked: HashSet<String>,
    /// DIDs we removed; their calls are refused even if they still hold a valid grant.
    pub blocked: HashSet<String>,
    /// Invite nonces already spent; a ticket adds one contact.
    pub redeemed: HashSet<String>,
    /// The ticket we hand out until someone redeems it, so the QR stays stable across launches.
    pub ticket: Option<String>,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn profile(&self) -> Result<Option<Profile>, Error> {
        read(&self.dir.join("profile.json"))
    }

    pub fn save_profile(&self, p: &Profile) -> Result<(), Error> {
        write(&self.dir.join("profile.json"), p)
    }

    /// A corrupt state file (power loss mid-write on a filesystem that reordered it) must not
    /// brick the app: it is set aside as `state.json.corrupt` and we start from empty state.
    /// Contacts are lost then, but the identity in `profile.json` is not.
    pub fn state(&self) -> Result<State, Error> {
        let path = self.dir.join("state.json");
        match read(&path) {
            Ok(s) => Ok(s.unwrap_or_default()),
            Err(Error::Io(e)) if path.exists() => {
                tracing::warn!("state.json unreadable ({e}); starting empty");
                let _ = fs::rename(&path, path.with_extension("json.corrupt"));
                Ok(State::default())
            }
            Err(e) => Err(e),
        }
    }

    pub fn save_state(&self, s: &State) -> Result<(), Error> {
        write(&self.dir.join("state.json"), s)
    }
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Write to a temp file, fsync it, rename over the old one, fsync the directory: after a
/// crash or power cut the file is either the old version or the new one, never a torn mix.
/// Matters most for `profile.json`, which holds the recovery phrase and device key.
fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}
