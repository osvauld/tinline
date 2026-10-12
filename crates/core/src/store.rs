//! What survives a restart, in one account's directory (`<root>/accounts/<id>/`, see
//! `accounts.rs`). Every JSON write goes to a temp file then renames over the old one, so a kill
//! mid-write leaves the previous state.
//!
//! `account.json` keeps the name, DID and device public key in the clear (so a lock screen and
//! the account switcher can show them) and the mnemonic and device secret only inside the
//! passphrase-sealed `vault` (see `vault.rs`, `docs/vault.md`). Installs from before the vault
//! have the old clear-text shape; they load as `Disk::Legacy` until `set_passphrase` converts them.
//!
//! State, call history and the device label live in `account.redb`, sealed under a key derived
//! from the vault's data key (`Backing::Sealed`), so they exist only while unlocked. A legacy
//! profile has no data key and keeps `state.json` / `calls.json` (`Backing::Json`) until converted.

use std::collections::{BTreeSet, HashMap, HashSet};
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
    /// This install's device ID (hex of 32 random bytes; U16's "device value", P9b): new when
    /// the account is created, linked or restored over this directory, kept when only the device
    /// key changes (endpoint rotation). Never derived from any key. The chat writer ID is
    /// `writer_id(DID, device ID)`. Absent in profiles from before P9b until the chat store
    /// first opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// A name edited while locked must be applied as a local profile edit on next unlock.
    #[serde(default)]
    pub profile_name_pending: bool,
    /// A newly linked/restored install must learn the account profile, not publish defaults
    /// that could overwrite the existing account's name or availability.
    #[serde(default)]
    pub profile_inherited: bool,
}

/// A fresh device ID (`ProfileV2::device_id`).
pub fn new_device_id() -> String {
    hex::encode(crate::chat::doc::new_device_value())
}

impl ProfileV2 {
    /// The device ID as bytes, if the profile has a well-formed one.
    pub fn device_id_bytes(&self) -> Option<[u8; 32]> {
        self.device_id.as_deref().and_then(|h| hex::decode(h).ok()).and_then(|b| b.try_into().ok())
    }
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

/// One callable device of a contact, with the relay it was last seen on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactDevice {
    pub device: [u8; 32],
    #[serde(default)]
    pub relay: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "RawContact")]
pub struct StoredContact {
    pub did: String,
    pub name: String,
    /// Calls dial all of them. Without a `device_list`: newest first (at most
    /// `proto::MAX_LIST_DEVICES`); with one: the list's order.
    pub devices: Vec<ContactDevice>,
    /// The newest signed device list we hold for them; once present it, not `note_device`,
    /// decides which devices we dial.
    pub device_list: Option<proto::SignedBlob>,
    /// Their message token for us (U13): opens chat with them. Replaced by a renewal that
    /// outlives it. A legacy (pre-P6) `call` grant counts as message + call.
    pub grant_from_them: proto::SignedGrant,
    /// Their call token for us, if they let us call (`None`: we can only message them).
    /// Replaced by a renewal that outlives it; dropped when they answer "calls not allowed".
    pub call_grant_from_them: Option<proto::SignedGrant>,
    /// Whether WE let them call us (we issued them a call token and have not taken it back).
    /// Ticket contacts: true; group chat-request contacts: false (U13). Synced to own devices.
    pub calls_allowed: bool,
    pub added_at: u64,
    /// Local-only name we gave them; `name` stays what they call themselves.
    pub alias: Option<String>,
    /// We compared safety numbers with them in person. Reset when their device changes.
    pub verified: bool,
}

impl StoredContact {
    pub fn has_device(&self, device: &[u8; 32]) -> bool {
        self.devices.iter().any(|d| &d.device == device)
    }

    pub fn device_keys(&self) -> Vec<[u8; 32]> {
        self.devices.iter().map(|d| d.device).collect()
    }

    pub fn relay_for(&self, device: &[u8; 32]) -> Option<&str> {
        self.devices.iter().find(|d| &d.device == device).and_then(|d| d.relay.as_deref())
    }
}

/// A device as older versions stored it (a bare key) or as now.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawDevice {
    Old([u8; 32]),
    New(ContactDevice),
}

/// Reads both shapes: before multi-device a contact had `devices: [key]` and one contact-wide
/// `relay`, which becomes the hint of the first (newest) device.
#[derive(Deserialize)]
struct RawContact {
    did: String,
    name: String,
    devices: Vec<RawDevice>,
    #[serde(default)]
    relay: Option<String>,
    #[serde(default)]
    device_list: Option<proto::SignedBlob>,
    grant_from_them: proto::SignedGrant,
    #[serde(default)]
    call_grant_from_them: Option<proto::SignedGrant>,
    #[serde(default = "yes")]
    calls_allowed: bool,
    added_at: u64,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    verified: bool,
}

fn yes() -> bool {
    true
}

impl From<RawContact> for StoredContact {
    fn from(r: RawContact) -> Self {
        let mut legacy_relay = r.relay;
        let devices = r
            .devices
            .into_iter()
            .map(|d| match d {
                RawDevice::Old(device) => ContactDevice { device, relay: legacy_relay.take() },
                RawDevice::New(d) => d,
            })
            .collect();
        StoredContact {
            did: r.did,
            name: r.name,
            devices,
            device_list: r.device_list,
            // U13 migration: a legacy `call` grant also lets us call (records from before P6
            // have no call slot and default to `calls_allowed`).
            call_grant_from_them: r
                .call_grant_from_them
                .or_else(|| proto::peek_grant(&r.grant_from_them).filter(|g| g.is_legacy()).map(|_| r.grant_from_them.clone())),
            grant_from_them: r.grant_from_them,
            calls_allowed: r.calls_allowed,
            added_at: r.added_at,
            alias: r.alias,
            verified: r.verified,
        }
    }
}

/// One install of this account, as our own registry knows it. Sealed with the rest of the state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnDevice {
    pub device: [u8; 32],
    pub attestation: proto::SignedAttestation,
    pub label: String,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub last_seen: u64,
    /// The relay it was last known on (our own entry is refreshed from the endpoint).
    #[serde(default)]
    pub relay: Option<String>,
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
    /// Ids of grants we issued and then took back. Not written today: tokens are taken back per
    /// DID with `cut_offs` (every renewal mints a new id, so ids would leak).
    pub revoked: HashSet<String>,
    /// Per holder DID: tokens we issued at or before these times (unix seconds) are refused
    /// (P6, U13). A block raises both, "disallow calls" the call one. Only ever raised; synced
    /// to own devices (max wins). Unblocking does not lower it: the contact needs fresh tokens.
    pub cut_offs: HashMap<String, CutOffs>,
    /// Tokens this device still has to deliver, per DID: capability names ("message", "call").
    /// Local (only the device that queued them delivers); fresh tokens are minted on delivery.
    pub token_outbox: HashMap<String, BTreeSet<String>>,
    /// DIDs we block (U19: the contact is kept). Everything from them is refused with the notice
    /// in `sent_notices`. Always the DIDs whose `sent_notices` entry is a block.
    pub blocked: HashSet<String>,
    /// The latest block / unblock notice we signed for each DID (`proto::sign_block_notice`);
    /// synced to own devices, which hand out the same notice instead of signing their own.
    pub sent_notices: HashMap<String, proto::SignedBlob>,
    /// Notices this device still has to deliver (local: only the device that signed one sends it).
    pub notice_outbox: HashSet<String>,
    /// The latest notice each DID sent us, verified on intake; a block there is "<name> blocked
    /// you". Kept after an unblock so an older notice replayed later is refused.
    pub notices: HashMap<String, HeldNotice>,
    /// Invite nonces already spent; a ticket adds one contact.
    pub redeemed: HashSet<String>,
    /// The ticket we hand out until someone redeems it, so the QR stays stable across launches.
    pub ticket: Option<String>,
    pub availability: AvailabilityState,
    /// Our own installs (task 34c: seeded with this device; linking fills it later).
    pub registry: Vec<OwnDevice>,
    /// The latest `DeviceList` we signed from the registry; sent in every call hello and accept,
    /// and pushed to contacts when its device set changes (U8, `devlist.rs`).
    pub own_list: Option<proto::SignedBlob>,
    /// Contacts this device still has to send its current `own_list` to (local: the device that
    /// saw the change pushes; the list is read fresh on each attempt).
    pub list_outbox: HashSet<String>,
    /// Contacts owed our current profile version, delivered on existing chat control sessions.
    pub profile_outbox: HashSet<String>,
    /// The device keys (text form, sorted) of the last list queued for every contact, so a change
    /// is pushed once even if a call re-signed the list first.
    pub pushed_list_keys: Vec<String>,
    /// One-time contact tickets we handed out by accepting a group chat request (U3), by
    /// invite nonce: only that DID may redeem one, and it gets message-only tokens (P6). Local:
    /// the ticket names this device's private endpoint.
    pub req_tickets: HashMap<String, ReqTicket>,
}

/// See `State::req_tickets`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReqTicket {
    pub did: String,
    /// Unix seconds.
    pub exp: u64,
}

/// See `State::cut_offs`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutOffs {
    pub message: u64,
    pub call: u64,
}

impl CutOffs {
    /// Raises each field to the other's (merge between own devices).
    pub fn merge(&mut self, other: &CutOffs) {
        self.message = self.message.max(other.message);
        self.call = self.call.max(other.call);
    }
}

/// A notice someone sent us, with the name we knew them by (their contact entry is deleted on a
/// block, U20, but the kept conversation still needs a name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldNotice {
    pub notice: proto::SignedBlob,
    pub name: String,
    /// From the verified notice: a block (true) or an unblock, and its `at` (unix ms).
    pub blocked: bool,
    pub at: u64,
}

/// Most calls kept in the call log; the oldest fall off.
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

/// Where state and the call log are written.
#[derive(Clone)]
pub enum Backing {
    /// Pre-vault install: `state.json` / `calls.json` in the account dir.
    Json(Store),
    /// `account.redb`, every value sealed under the account key.
    Sealed(storage::Sealed),
}

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

/// Opens (creating) the account's sealed store. It is keyed by the recovery `phrase` (see
/// `rekey.rs`); `dek` lets a store from before that change be re-keyed on the way.
pub fn open_sealed(dir: &Path, phrase: &str, dek: Option<&[u8; 32]>) -> Result<storage::Sealed, Error> {
    let path = dir.join("account.redb");
    let db = open_waiting(&path)?;
    set_private(&path);
    crate::rekey::open(
        db,
        crate::rekey::key_from_phrase(crate::rekey::Which::Account, phrase),
        dek.map(|d| crate::rekey::old_key(crate::rekey::Which::Account, d)),
    )
}

fn io(e: storage::StorageError) -> Error {
    Error::Io(e.to_string())
}

impl Backing {
    pub fn save_state(&self, s: &State) -> Result<(), Error> {
        match self {
            Backing::Json(store) => store.save_state(s),
            Backing::Sealed(db) => db.put(STATE_KEY, &zeroizing_json(s)?).map_err(io),
        }
    }

    pub fn save_calls(&self, calls: &[CallRecord]) -> Result<(), Error> {
        match self {
            Backing::Json(store) => store.save_calls(calls),
            Backing::Sealed(db) => db.put(CALLS_KEY, &zeroizing_json(&calls)?).map_err(io),
        }
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

/// Opens the sealed store of the account in `store`'s dir. First unlock after a migration or
/// a legacy conversion also imports `state.json` / `calls.json`: both land in one transaction,
/// and only then are the JSON files (and their `.corrupt` / `.tmp` leftovers) scrubbed.
/// Anything that fails leaves the JSON in place. A record that does not open is an error, never
/// an empty state.
pub fn load_sealed(store: &Store, phrase: &str, dek: Option<&[u8; 32]>) -> Result<(storage::Sealed, Loaded), Error> {
    let db = open_sealed(store.dir(), phrase, dek)?;
    let mut ops = Vec::new();
    if db.get(STATE_KEY).map_err(io)?.is_none() && store.dir().join("state.json").exists() {
        ops.push(db.put_op(STATE_KEY, &zeroizing_json(&store.state()?)?).map_err(io)?);
    }
    if db.get(CALLS_KEY).map_err(io)?.is_none() && store.dir().join("calls.json").exists() {
        ops.push(db.put_op(CALLS_KEY, &zeroizing_json(&store.calls()?)?).map_err(io)?);
    }
    if !ops.is_empty() {
        db.apply(&ops).map_err(io)?;
    }
    store.remove_json_state();
    let state = match db.get(STATE_KEY).map_err(io)? {
        Some(b) => serde_json::from_slice(&b)?,
        None => State::default(),
    };
    let calls = match db.get(CALLS_KEY).map_err(io)? {
        Some(b) => serde_json::from_slice(&b)?,
        None => Vec::new(),
    };
    let device_label = db.get(LABEL_KEY).map_err(io)?.and_then(|b| String::from_utf8(b).ok());
    Ok((db, Loaded { state, calls, device_label }))
}

/// Seals the in-memory state and call log into a new store, e.g. when a legacy profile gets its
/// data key, and scrubs the JSON they came from.
pub fn seal_into(store: &Store, phrase: &str, dek: &[u8; 32], state: &State, calls: &[CallRecord], label: Option<&str>) -> Result<storage::Sealed, Error> {
    let db = open_sealed(store.dir(), phrase, Some(dek))?;
    let mut ops = vec![
        db.put_op(STATE_KEY, &zeroizing_json(state)?).map_err(io)?,
        db.put_op(CALLS_KEY, &zeroizing_json(&calls)?).map_err(io)?,
    ];
    if let Some(l) = label {
        ops.push(db.put_op(LABEL_KEY, l.as_bytes()).map_err(io)?);
    }
    db.apply(&ops).map_err(io)?;
    store.remove_json_state();
    Ok(db)
}

pub fn save_label(db: &storage::Sealed, label: &str) -> Result<(), Error> {
    db.put(LABEL_KEY, label.as_bytes()).map_err(io)
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

    /// The JSON state files and their leftovers, once their content is sealed. Overwritten with
    /// zeros first (best effort: a copy-on-write filesystem or flash wear-levelling may keep old
    /// blocks).
    pub fn remove_json_state(&self) {
        for name in ["state.json", "calls.json", "state.json.corrupt", "calls.json.corrupt", "state.tmp", "calls.tmp"] {
            scrub_remove(&self.dir.join(name));
        }
    }

    /// After the legacy clear-text profile was converted: nothing of it may linger.
    pub fn remove_legacy_leftovers(&self) {
        for name in ["profile.json.bak", "profile.bak", "profile.tmp"] {
            scrub_remove(&self.dir.join(name));
        }
    }

    pub fn profile(&self) -> Result<Option<Disk>, Error> {
        read_profile(&self.dir)
    }

    pub fn save_profile(&self, p: &Disk) -> Result<(), Error> {
        let path = self.dir.join("account.json");
        match p {
            Disk::V2(p) => write(&path, p),
            Disk::Legacy(p) => write(&path, p),
        }
    }

    /// A state file that does not parse (power loss mid-write on a filesystem that reordered
    /// it) must not brick the app: it is set aside as `state.json.corrupt` and we start from
    /// empty state. Contacts are lost then, but the identity in `account.json` is not. An IO
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

/// The account dir's `account.json`, either shape.
pub fn read_profile(dir: &Path) -> Result<Option<Disk>, Error> {
    read_disk(&dir.join("account.json"))
}

/// A profile file of either shape (`profile.json` of the single-account layout included).
pub fn read_disk(path: &Path) -> Result<Option<Disk>, Error> {
    let Some(v): Option<serde_json::Value> = read(path)? else {
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

fn scrub_remove(path: &Path) {
    use std::io::Write;
    if let Ok(meta) = fs::metadata(path)
        && meta.is_file()
        && let Ok(mut f) = fs::OpenOptions::new().write(true).open(path)
    {
        let _ = f.write_all(&vec![0u8; meta.len() as usize]);
        let _ = f.sync_all();
    }
    let _ = fs::remove_file(path);
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

    // KIND: invariant (P10)
    // A store file another owner still holds for a moment (the node before a restart) opens once
    // it is let go, instead of failing at once; one held for good still fails, after ~2 s.
    #[test]
    fn a_store_still_held_opens_once_released() {
        let dir = tmp("held");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat.redb");
        let held = storage::Store::open(&path).unwrap();
        assert!(storage::Store::open(&path).is_err(), "redb refuses a second owner");
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            drop(held);
        });
        open_waiting(&path).unwrap();
        t.join().unwrap();
        let _again = open_waiting(&path).unwrap();
        let start = std::time::Instant::now();
        assert!(open_waiting(&path).is_err());
        assert!(start.elapsed() >= std::time::Duration::from_secs(1));
        let _ = fs::remove_dir_all(&dir);
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
        let h = History::new(store.calls().unwrap(), Some(Backing::Json(store.clone())));
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
        h.save().unwrap();
        let back = History::new(store.calls().unwrap(), None);
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

    #[test]
    fn old_contact_records_load_with_the_relay_on_the_first_device() {
        let old = serde_json::json!({
            "did": "did:key:zX", "name": "N",
            "devices": [vec![1u8; 32], vec![2u8; 32]],
            "relay": "https://r.example/",
            "grant_from_them": {"payload": "p", "signature": "s"},
            "added_at": 5
        });
        let c: StoredContact = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(c.devices.len(), 2);
        assert_eq!((c.devices[0].device, c.devices[0].relay.as_deref()), ([1u8; 32], Some("https://r.example/")));
        assert_eq!((c.devices[1].device, c.devices[1].relay.clone()), ([2u8; 32], None));
        assert!(c.device_list.is_none() && !c.verified && c.alias.is_none());
        // Before P6: calls allowed by default; "p" is no grant at all, so nothing to migrate.
        assert!(c.calls_allowed && c.call_grant_from_them.is_none());
        // Round trip in the new shape, and as part of a whole old State.
        let again: StoredContact = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(again.devices, c.devices);
        let st: State = serde_json::from_value(serde_json::json!({"contacts": [old], "blocked": ["b"]})).unwrap();
        assert_eq!(st.contacts[0].devices.len(), 2);
        assert!(st.registry.is_empty() && st.own_list.is_none());
        // No relay, and a contact with no devices at all, still load.
        let bare = serde_json::json!({"did":"d","name":"n","devices":[],"relay":null,"grant_from_them":{"payload":"p","signature":"s"},"added_at":1});
        assert!(serde_json::from_value::<StoredContact>(bare).unwrap().devices.is_empty());
    }

    // KIND: invariant (P6, U13 migration)
    // A record from before separate tokens holds one legacy `call` grant: it loads with that
    // grant in both slots (message and call) and calls allowed; a record with a version-2
    // message token gets no call slot from it.
    #[test]
    fn a_legacy_call_grant_migrates_to_message_and_call() {
        let (them, _) = identity::generate();
        let legacy = proto::issue_legacy_grant_for_test(&them, "did:key:me", 1, 100);
        let rec = |g: &proto::SignedGrant| serde_json::json!({"did":"d","name":"n","devices":[],"grant_from_them":g,"added_at":1});
        let c: StoredContact = serde_json::from_value(rec(&legacy)).unwrap();
        assert_eq!(c.call_grant_from_them.as_ref(), Some(&legacy));
        assert!(c.calls_allowed);
        let msg = proto::issue_token(&them, "did:key:me", proto::Cap::Message, 1, 100);
        let c: StoredContact = serde_json::from_value(rec(&msg)).unwrap();
        assert!(c.call_grant_from_them.is_none());
        // The new shape round-trips.
        let again: StoredContact = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert!(again.call_grant_from_them.is_none() && again.calls_allowed);
    }
}
