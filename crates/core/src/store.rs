//! What survives a restart, as JSON files in the app's private data dir. Every write goes to a
//! temp file then renames over the old one, so a kill mid-write leaves the previous state.
//!
//! `profile.json` keeps the name, DID and device public key in the clear (so a lock screen can
//! show them) and the mnemonic and device secret only inside the passphrase-sealed `vault`
//! (see `vault.rs`, `docs/vault.md`). Installs from before the vault have the old clear-text
//! shape; they load as `Disk::Legacy` until `set_passphrase` converts them.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::Error;

/// Current on-disk profile (`"version": 2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileV2 {
    pub version: u32,
    pub name: String,
    pub did: String,
    pub device_public: [u8; 32],
    pub vault: crate::vault::Vault,
}

#[derive(Clone)]
pub enum Disk {
    V2(ProfileV2),
    Legacy(Profile),
}

/// The old clear-text profile; also what an unlocked identity is loaded from.
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

    pub fn profile(&self) -> Result<Option<Disk>, Error> {
        let Some(v): Option<serde_json::Value> = read(&self.dir.join("profile.json"))? else {
            return Ok(None);
        };
        if v.get("mnemonic").is_some() {
            Ok(Some(Disk::Legacy(serde_json::from_value(v)?)))
        } else {
            let p: ProfileV2 = serde_json::from_value(v)?;
            if p.version != 2 {
                return Err(Error::Io(format!("unknown profile version {}", p.version)));
            }
            Ok(Some(Disk::V2(p)))
        }
    }

    pub fn save_profile(&self, p: &Disk) -> Result<(), Error> {
        let path = self.dir.join("profile.json");
        match p {
            Disk::V2(p) => write(&path, p),
            Disk::Legacy(p) => write(&path, p),
        }
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
