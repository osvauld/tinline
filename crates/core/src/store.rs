//! What survives a restart, in one account's directory (`<root>/accounts/<id>/`, see
//! `accounts.rs`). Every JSON write goes to a temp file then renames over the old one, so a kill
//! mid-write leaves the previous state.
//!
//! `account.json` keeps the name, DID and device public key in the clear (so a lock screen and
//! the account switcher can show them) and the mnemonic and device secret only inside the
//! passphrase-sealed `vault` (see `vault.rs`, `docs/vault.md`).
//!
//! State, call history and the device label live in `account.redb`, sealed under a key derived
//! from the recovery phrase (`Backing::Sealed`, see `rekey.rs`), so they exist only while unlocked.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

mod models;
pub use self::models::*;


/// Where state and the call log are written.
#[derive(Clone)]
pub enum Backing {
    /// `account.redb`, every value sealed under the account key.
    Sealed(storage::Sealed),
}

/// Cap for decoding a binary row our own store wrote (disk garbage is still bounded).
pub(crate) const MAX_ROW: usize = 1 << 20;

const STATE_KEY: &str = "state";
const CALLS_KEY: &str = "calls";
const LABEL_KEY: &str = "device_label";

/// Opens (creating) a store file. A previous owner of the file (the node of a restart, or a task
/// of an account just left) may still hold it for a moment: retried for up to about 2 s.
/// Opens of each store file in this process, and how many of them had to wait for the file
/// (another owner still held it: "Database already open"). For `Node::store_stats_for_test`;
/// keyed by path so tests running side by side in one process do not mix.
static OPENS: parking_lot::Mutex<Option<HashMap<PathBuf, (u64, u64)>>> = parking_lot::Mutex::new(None);

pub(crate) fn note_open(path: &Path, waited: bool) {
    let mut m = OPENS.lock();
    let e = m.get_or_insert_with(HashMap::new).entry(path.to_path_buf()).or_default();
    e.0 += 1;
    if waited {
        e.1 += 1;
        tracing::warn!("{}: had to wait for a previous owner to let go of the file", path.display());
    }
}

/// (opens, opens that waited) of every store file under `dir`.
pub(crate) fn open_stats(dir: &Path) -> (u64, u64) {
    let m = OPENS.lock();
    m.iter().flatten().filter(|(p, _)| p.starts_with(dir)).fold((0, 0), |a, (_, v)| (a.0 + v.0, a.1 + v.1))
}

pub fn open_waiting(path: &Path) -> Result<storage::Store, Error> {
    let mut tries = 0;
    loop {
        match storage::Store::open(path) {
            Ok(db) => {
                note_open(path, tries > 0);
                return Ok(db);
            }
            Err(e) if tries >= 20 => return Err(Error::Io(e.to_string())),
            Err(_) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// Opens (creating) the account's sealed store, keyed by the recovery `phrase` (`rekey.rs`).
pub fn open_sealed(dir: &Path, phrase: &str) -> Result<storage::Sealed, Error> {
    let path = dir.join("account.redb");
    let db = open_waiting(&path)?;
    set_private(&path);
    crate::rekey::open(db, crate::rekey::key_from_phrase(crate::rekey::Which::Account, phrase))
}

impl Backing {
    pub fn save_state(&self, s: &State) -> Result<(), Error> {
        let Backing::Sealed(db) = self;
        db.put(STATE_KEY, &zeroizing_json(s)?).map_err(Error::from)
    }

    pub fn save_calls(&self, calls: &[CallRecord]) -> Result<(), Error> {
        let Backing::Sealed(db) = self;
        db.put(CALLS_KEY, &zeroizing_json(&calls)?).map_err(Error::from)
    }
}

fn zeroizing_json<T: Serialize + ?Sized>(v: &T) -> Result<zeroize::Zeroizing<Vec<u8>>, Error> {
    Ok(zeroize::Zeroizing::new(serde_json::to_vec(v)?))
}

/// What an unlock found in the account's sealed store.
pub struct Loaded {
    pub state: State,
    pub calls: Vec<CallRecord>,
    pub device_label: Option<String>,
}

/// Opens the sealed store of the account in `store`'s dir. A record that does not open is an
/// error, never an empty state.
pub fn load_sealed(store: &Store, phrase: &str) -> Result<(storage::Sealed, Loaded), Error> {
    let db = open_sealed(store.dir(), phrase)?;
    let state = match db.get(STATE_KEY)? {
        Some(b) => serde_json::from_slice(&b)?,
        None => State::default(),
    };
    let calls = match db.get(CALLS_KEY)? {
        Some(b) => serde_json::from_slice(&b)?,
        None => Vec::new(),
    };
    let device_label = db.get(LABEL_KEY)?.and_then(|b| String::from_utf8(b).ok());
    Ok((db, Loaded { state, calls, device_label }))
}

/// Seals the in-memory state and call log into the account's store (a new account, a restore).
pub fn seal_into(store: &Store, phrase: &str, state: &State, calls: &[CallRecord], label: Option<&str>) -> Result<storage::Sealed, Error> {
    let db = open_sealed(store.dir(), phrase)?;
    let mut ops = vec![
        db.put_op(STATE_KEY, &zeroizing_json(state)?)?,
        db.put_op(CALLS_KEY, &zeroizing_json(&calls)?)?,
    ];
    if let Some(l) = label {
        ops.push(db.put_op(LABEL_KEY, l.as_bytes())?);
    }
    db.apply(&ops)?;
    Ok(db)
}

pub fn save_label(db: &storage::Sealed, label: &str) -> Result<(), Error> {
    db.put(LABEL_KEY, label.as_bytes()).map_err(Error::from)
}

/// The call log, newest first. Writes are serialised and ordered like state's. One per unlocked
/// account, owning that account's backing, so a task that outlives a switch can only ever write
/// to the account it started under.
pub struct History {
    calls: parking_lot::Mutex<Vec<CallRecord>>,
    writing: parking_lot::Mutex<()>,
    backing: parking_lot::Mutex<Option<Backing>>,
}

impl History {
    pub fn new(calls: Vec<CallRecord>, backing: Option<Backing>) -> Self {
        Self { calls: parking_lot::Mutex::new(calls), writing: Default::default(), backing: parking_lot::Mutex::new(backing) }
    }

    /// Nothing to show and nowhere to write: while locked, or before an identity exists.
    pub fn empty() -> Self {
        Self::new(Vec::new(), None)
    }

    /// For an identity that was committed after it started collecting records.
    pub fn attach(&self, backing: Backing) {
        *self.backing.lock() = Some(backing);
    }

    pub fn push(&self, rec: CallRecord) {
        let mut c = self.calls.lock();
        c.insert(0, rec);
        c.truncate(MAX_HISTORY);
    }

    /// Newest first; `did` filters to one person.
    pub fn list(&self, did: Option<&str>, limit: usize) -> Vec<CallRecord> {
        let c = self.calls.lock();
        c.iter().filter(|r| did.is_none_or(|d| r.peer_did == d)).take(limit).cloned().collect()
    }

    /// Swaps in the merged log of an own-device sync.
    pub fn replace(&self, calls: Vec<CallRecord>) {
        *self.calls.lock() = calls;
    }

    #[cfg(test)]
    pub fn remove_peer(&self, did: &str) {
        self.calls.lock().retain(|r| r.peer_did != did);
    }

    pub fn snapshot(&self) -> Vec<CallRecord> {
        self.calls.lock().clone()
    }

    /// Without a backing (not yet committed) there is nothing to write yet.
    pub fn save(&self) -> Result<(), Error> {
        let _w = self.writing.lock();
        let snapshot = self.calls.lock().clone();
        match self.backing.lock().clone() {
            Some(b) => b.save_calls(&snapshot),
            None => Ok(()),
        }
    }
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let store = Self { dir };
        store.tighten_permissions();
        store.remove_stale();
        Ok(store)
    }

    /// Only the app's own user may read the data dir: it holds the (sealed) identity and the
    /// contact list. Fixes up installs created before this was enforced.
    fn tighten_permissions(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let chmod = |p: &Path, mode: u32| {
                if let Err(e) = fs::set_permissions(p, fs::Permissions::from_mode(mode)) {
                    tracing::warn!("chmod {}: {e}", p.display());
                }
            };
            chmod(&self.dir, 0o700);
            for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
                if entry.file_type().is_ok_and(|t| t.is_file()) {
                    chmod(&entry.path(), 0o600);
                }
            }
        }
    }

    /// Leftovers of a write that was killed before its rename.
    fn remove_stale(&self) {
        for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            if entry.path().extension().is_some_and(|e| e == "tmp") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    pub fn profile(&self) -> Result<Option<Disk>, Error> {
        read_profile(&self.dir)
    }

    pub fn save_profile(&self, p: &Disk) -> Result<(), Error> {
        let path = self.dir.join("account.json");
        let Disk::V2(p) = p;
        write(&path, p)
    }
}

/// The account dir's `account.json`.
pub fn read_profile(dir: &Path) -> Result<Option<Disk>, Error> {
    let Some(p): Option<ProfileV2> = read(&dir.join("account.json"))? else {
        return Ok(None);
    };
    if p.version != 2 {
        return Err(Error::Io(format!("unknown profile version {}", p.version)));
    }
    Ok(Some(Disk::V2(p)))
}

pub(crate) fn set_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
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
/// Matters most for `account.json`, which holds the recovery phrase and device key.
fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
