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
use zeroize::ZeroizeOnDrop;

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
#[derive(Clone, Serialize, Deserialize, ZeroizeOnDrop)]
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
    /// Local-only name we gave them; `name` stays what they call themselves.
    #[serde(default)]
    pub alias: Option<String>,
    /// We compared safety numbers with them in person. Reset when their device changes.
    #[serde(default)]
    pub verified: bool,
}

/// "Not now": calls are turned away (and not shown) until `until`, or until switched back on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AvailabilityState {
    pub unavailable: bool,
    pub until: Option<u64>,
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
    pub availability: AvailabilityState,
}

/// Most calls kept in `calls.json`; the oldest fall off.
const MAX_HISTORY: usize = 500;

/// One finished call, as the UI lists it. `reason` is one of the stable end reasons in
/// `docs/protocol.md`.
#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct CallRecord {
    pub call_id: String,
    pub peer_did: String,
    /// Their name when the call happened (our alias for them if we had one).
    pub peer_name: String,
    pub incoming: bool,
    /// Unix seconds when the call started (dialing, or the first ring).
    pub started_at: u64,
    /// Seconds from answer to end; 0 if it never became active.
    pub duration_secs: u32,
    pub reason: String,
    /// Media went over a direct path rather than a relay (as last seen).
    pub direct: bool,
    /// Incoming and we never answered it (not one we declined, nor one turned away while
    /// unavailable).
    pub missed: bool,
}

/// The call log, newest first, in `calls.json`. Writes are serialised and ordered like state's.
#[derive(Default)]
pub struct History {
    calls: parking_lot::Mutex<Vec<CallRecord>>,
    writing: parking_lot::Mutex<()>,
}

impl History {
    pub fn new(calls: Vec<CallRecord>) -> Self {
        Self { calls: parking_lot::Mutex::new(calls), writing: Default::default() }
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

    pub fn remove_peer(&self, did: &str) {
        self.calls.lock().retain(|r| r.peer_did != did);
    }

    pub fn save(&self, store: &Store) -> Result<(), Error> {
        let _w = self.writing.lock();
        let snapshot = self.calls.lock().clone();
        store.save_calls(&snapshot)
    }
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
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

    /// After the legacy clear-text profile was converted: nothing of it may linger.
    pub fn remove_legacy_leftovers(&self) {
        for name in ["profile.json.bak", "profile.bak", "profile.tmp"] {
            let _ = fs::remove_file(self.dir.join(name));
        }
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

    /// A state file that does not parse (power loss mid-write on a filesystem that reordered
    /// it) must not brick the app: it is set aside as `state.json.corrupt` and we start from
    /// empty state. Contacts are lost then, but the identity in `profile.json` is not. An IO
    /// error (permissions, a flaky disk) is returned instead: the file may be fine.
    pub fn state(&self) -> Result<State, Error> {
        let path = self.dir.join("state.json");
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
            Err(e) => return Err(e.into()),
        };
        match serde_json::from_slice(&bytes) {
            Ok(s) => Ok(s),
            Err(e) => {
                tracing::warn!("state.json does not parse ({e}); starting empty");
                let _ = fs::rename(&path, path.with_extension("json.corrupt"));
                Ok(State::default())
            }
        }
    }

    pub fn save_state(&self, s: &State) -> Result<(), Error> {
        write(&self.dir.join("state.json"), s)
    }

    /// The call log; one that does not parse is set aside like `state.json` and starts empty.
    pub fn calls(&self) -> Result<Vec<CallRecord>, Error> {
        let path = self.dir.join("calls.json");
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        match serde_json::from_slice(&bytes) {
            Ok(c) => Ok(c),
            Err(e) => {
                tracing::warn!("calls.json does not parse ({e}); starting empty");
                let _ = fs::rename(&path, path.with_extension("json.corrupt"));
                Ok(Vec::new())
            }
        }
    }

    pub fn save_calls(&self, calls: &[CallRecord]) -> Result<(), Error> {
        write(&self.dir.join("calls.json"), &calls)
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
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("p2pcore-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_and_old_ones_are_fixed() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tmp("modes");
        // An install from before: loose dir and file modes.
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(dir.join("state.json"), b"{}").unwrap();
        fs::set_permissions(dir.join("state.json"), fs::Permissions::from_mode(0o644)).unwrap();
        let store = Store::open(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("state.json")), 0o600);
        store.save_state(&State::default()).unwrap();
        assert_eq!(mode(&dir.join("state.json")), 0o600);
        let top = tmp("fresh");
        let fresh = top.join("a");
        let store = Store::open(&fresh).unwrap();
        store.save_state(&State::default()).unwrap();
        assert_eq!(mode(&fresh), 0o700);
        assert_eq!(mode(&fresh.join("state.json")), 0o600);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&top);
    }

    #[test]
    fn stale_temp_files_are_removed_on_open() {
        let dir = tmp("stale");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("profile.tmp"), b"secret").unwrap();
        fs::write(dir.join("state.tmp"), b"x").unwrap();
        fs::write(dir.join("state.json"), b"{}").unwrap();
        Store::open(&dir).unwrap();
        assert!(!dir.join("profile.tmp").exists() && !dir.join("state.tmp").exists());
        assert!(dir.join("state.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_is_capped_newest_first_and_private() {
        let dir = tmp("calls");
        let store = Store::open(&dir).unwrap();
        let h = History::new(store.calls().unwrap());
        let rec = |i: usize, did: &str| CallRecord {
            call_id: i.to_string(),
            peer_did: did.into(),
            peer_name: "x".into(),
            incoming: false,
            started_at: i as u64,
            duration_secs: 0,
            reason: "hangup_local".into(),
            direct: false,
            missed: false,
        };
        for i in 0..MAX_HISTORY + 5 {
            h.push(rec(i, if i % 2 == 0 { "a" } else { "b" }));
        }
        h.save(&store).unwrap();
        let back = History::new(store.calls().unwrap());
        assert_eq!(back.list(None, 10_000).len(), MAX_HISTORY);
        assert_eq!(back.list(None, 1)[0].call_id, (MAX_HISTORY + 4).to_string());
        assert!(back.list(Some("a"), 10_000).iter().all(|r| r.peer_did == "a"));
        back.remove_peer("a");
        assert!(back.list(Some("a"), 10).is_empty() && !back.list(Some("b"), 10).is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("calls.json")).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        fs::write(dir.join("calls.json"), b"{ nope").unwrap();
        assert!(store.calls().unwrap().is_empty());
        assert!(dir.join("calls.json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_parse_error_quarantines_state() {
        let dir = tmp("corrupt");
        let store = Store::open(&dir).unwrap();
        fs::write(dir.join("state.json"), b"{ not json").unwrap();
        assert!(store.state().unwrap().contacts.is_empty());
        assert!(dir.join("state.json.corrupt").exists());
        // Something that cannot be read as a file is an IO error, and is left alone.
        fs::create_dir(dir.join("state.json")).unwrap();
        assert!(matches!(store.state(), Err(Error::Io(_))));
        assert!(dir.join("state.json").is_dir());
        let _ = fs::remove_dir_all(&dir);
    }
}
