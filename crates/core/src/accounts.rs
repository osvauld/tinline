//! The accounts on disk: one directory per account, a pointer to the selected one.
//!
//! ```text
//! <root>/accounts/<id>/account.json   clear: version, name, did, device_public, vault
//! <root>/accounts/<id>/account.redb   sealed: state, call history, device label
//! <root>/accounts/<id>/chat.redb, blobs/
//! <root>/current                      the <id> of the selected account
//! ```
//!
//! `<id>` is the DID's multibase part. This module only deals with directories and the clear
//! `account.json`; what is inside the sealed store is `store.rs`. It also moves a single-account
//! install (`profile.json` & co. in the root) into this layout.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::store::{Disk, read_disk, read_profile};

/// The DID's multibase part, as a directory name. Refuses anything that is not a DID, so a name
/// can never reach outside `accounts/`.
pub fn id_of(did: &str) -> Result<String, Error> {
    identity::public_key_from_did(did).ok_or_else(|| Error::Io("invalid account DID".into()))?;
    let id = did.strip_prefix("did:key:").ok_or_else(|| Error::Io("invalid account DID".into()))?;
    valid_id(id)?;
    Ok(id.to_string())
}

fn valid_id(id: &str) -> Result<(), Error> {
    if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Error::Io("invalid account id".into()));
    }
    Ok(())
}

/// What the lock screen and the switcher need, read from the clear `account.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub did: String,
    pub name: String,
    pub has_passphrase: bool,
}

pub fn entry_of(id: &str, disk: &Disk) -> Result<Entry, Error> {
    let (did, name, has_passphrase) = match disk {
        Disk::V2(p) => (p.did.clone(), p.name.clone(), p.vault.has_passphrase()),
        Disk::Legacy(p) => {
            let id = identity::recover(&p.mnemonic).map_err(|_| Error::BadPhrase)?;
            (id.did().to_string(), p.name.clone(), false)
        }
    };
    if id_of(&did)? != id {
        return Err(Error::Io("account directory does not match its identity".into()));
    }
    Ok(Entry { id: id.to_string(), did, name, has_passphrase })
}

/// Files of the old single-account layout that move into the account dir. `profile.json` goes
/// last (as `account.json`): while it is still in the root the move is not finished.
const MOVED: [&str; 9] = [
    "chat.redb",
    "blobs",
    "state.json",
    "calls.json",
    "state.json.corrupt",
    "calls.json.corrupt",
    "profile.json.bak",
    "profile.bak",
    "profile.tmp",
];
const MIGRATING: &str = ".migrating";

pub struct AccountDirs {
    root: PathBuf,
}

impl AccountDirs {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, Error> {
        let this = Self { root: root.into() };
        for d in [this.root.clone(), this.accounts()] {
            fs::create_dir_all(&d)?;
            private_dir(&d);
        }
        Ok(this)
    }

    fn accounts(&self) -> PathBuf {
        self.root.join("accounts")
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf, Error> {
        valid_id(id)?;
        Ok(self.accounts().join(id))
    }

    pub fn exists(&self, id: &str) -> Result<bool, Error> {
        Ok(self.dir(id)?.exists())
    }

    /// Every account with a readable `account.json`, by id. One that does not parse is skipped
    /// (and left alone), not fatal: the others must stay usable.
    pub fn list(&self) -> Result<Vec<Entry>, Error> {
        let mut out = Vec::new();
        for e in fs::read_dir(self.accounts())? {
            let e = e?;
            let Some(id) = e.file_name().to_str().map(str::to_string) else { continue };
            if valid_id(&id).is_err() || !e.file_type()?.is_dir() {
                continue;
            }
            match read_profile(&e.path()).and_then(|d| d.ok_or(Error::NotFound)).and_then(|d| entry_of(&id, &d)) {
                Ok(entry) => out.push(entry),
                Err(err) => tracing::warn!("account {id}: {err}"),
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// The selected account, if the pointer is valid and its directory exists.
    pub fn current(&self) -> Option<String> {
        let id = fs::read_to_string(self.root.join("current")).ok()?.trim().to_string();
        (valid_id(&id).is_ok() && self.accounts().join(&id).join("account.json").exists()).then_some(id)
    }

    pub fn select(&self, id: &str) -> Result<(), Error> {
        valid_id(id)?;
        write_atomic(&self.root.join("current"), id.as_bytes())
    }

    pub fn deselect(&self) -> Result<(), Error> {
        match fs::remove_file(self.root.join("current")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// A new, empty account dir. Fails with `AccountExists` rather than ever touching one that is
    /// already there.
    pub fn create(&self, id: &str) -> Result<PathBuf, Error> {
        let dir = self.dir(id)?;
        match fs::create_dir(&dir) {
            Ok(()) => {
                private_dir(&dir);
                Ok(dir)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::AccountExists),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes the account's whole directory. The caller makes sure it is not the open one.
    pub fn remove(&self, id: &str) -> Result<(), Error> {
        let dir = self.dir(id)?;
        if !dir.exists() {
            return Err(Error::NotFound);
        }
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// Moves a single-account install into `accounts/<id>/`. Only renames, so nothing is ever
    /// copied or deleted, and the sequence can be repeated after a crash at any point:
    ///
    /// 1. everything but the profile goes into `accounts/<id>.migrating/`;
    /// 2. `profile.json` goes there last, as `account.json`;
    /// 3. `current` is written, then the directory is renamed to its final name.
    ///
    /// Any error is returned with the remaining files exactly where they were.
    pub fn migrate(&self) -> Result<(), Error> {
        let old = self.root.join("profile.json");
        if old.exists() {
            let disk = read_profile_file(&old)?;
            let id = id_of(&entry_did(&disk)?)?;
            if self.dir(&id)?.exists() {
                return Err(Error::Io(format!("account {id} already exists; the old files were left in place")));
            }
            let tmp = self.accounts().join(format!("{id}{MIGRATING}"));
            // Everything is checked before the first rename, so an error moves nothing.
            let movable: Vec<&str> = MOVED.into_iter().filter(|n| self.root.join(n).symlink_metadata().is_ok()).collect();
            for name in &movable {
                if tmp.join(name).symlink_metadata().is_ok() {
                    return Err(Error::Io(format!("{name} exists in both the old and the new place")));
                }
            }
            if tmp.join("account.json").exists() {
                return Err(Error::Io("interrupted migration has an account.json already".into()));
            }
            fs::create_dir_all(&tmp)?;
            private_dir(&tmp);
            for name in movable {
                fs::rename(self.root.join(name), tmp.join(name))?;
            }
            sync_dir(&self.root);
            fs::rename(&old, tmp.join("account.json"))?;
            sync_dir(&tmp);
        }
        for e in fs::read_dir(self.accounts())? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(MIGRATING) else { continue };
            if valid_id(id).is_err() || !e.path().join("account.json").exists() {
                continue;
            }
            let fin = self.dir(id)?;
            if fin.exists() {
                return Err(Error::Io(format!("account {id} already exists; {name} was left in place")));
            }
            // The pointer first: a crash between the two steps resumes here, whereas a final
            // dir without a pointer would look like an install with no account selected.
            if self.current().is_none() {
                self.select(id)?;
            }
            fs::rename(e.path(), &fin)?;
            sync_dir(&self.accounts());
        }
        Ok(())
    }
}

fn entry_did(disk: &Disk) -> Result<String, Error> {
    Ok(match disk {
        Disk::V2(p) => p.did.clone(),
        Disk::Legacy(p) => identity::recover(&p.mnemonic).map_err(|_| Error::BadPhrase)?.did().to_string(),
    })
}

fn read_profile_file(path: &Path) -> Result<Disk, Error> {
    read_disk(path)?.ok_or(Error::NotFound)
}

fn private_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn sync_dir(path: &Path) {
    if let Ok(d) = fs::File::open(path) {
        let _ = d.sync_all();
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        sync_dir(dir);
    }
    Ok(())
}
