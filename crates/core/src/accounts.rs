//! Account-scoped sealed storage. Platform/Node integration and legacy migration follow separately.
//! Only opaque IDs and the key envelope are public on disk; labels and device secrets are sealed.
use std::path::PathBuf;

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    Error,
    vault::{self, Dek, Secrets, Vault},
};

const HEADER: &str = "identity/envelope";
const IDENTITY: &str = "account/identity";
const MAX_RECORD: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    vault: Vault,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    name: String,
    device_label: String,
}

/// A session capability. Old capabilities are rejected after locking or switching accounts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSession {
    pub did: String,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountInfo {
    pub did: String,
    pub name: String,
    pub device_label: String,
    pub device_public: [u8; 32],
}

struct Active {
    session: AccountSession,
    store: storage::Store,
    dek: Dek,
    info: AccountInfo,
}

#[derive(Default)]
struct State {
    generation: u64,
    active: Option<Active>,
}

/// Many accounts on disk, one unlocked account in memory. No network or platform key storage.
/// This first API requires a passphrase so creation cannot lose its sole platform unlock key.
pub struct Accounts {
    dir: PathBuf,
    state: Mutex<State>,
}

fn io(e: impl std::fmt::Display) -> Error {
    Error::Io(e.to_string())
}

fn filename(did: &str) -> Result<String, Error> {
    identity::public_key_from_did(did).ok_or_else(|| io("invalid account DID"))?;
    Ok(format!(
        "{}.redb",
        did.strip_prefix("did:key:")
            .ok_or_else(|| io("invalid DID"))?
    ))
}

fn record_key(name: &str) -> Result<String, Error> {
    if name.is_empty()
        || name.len() > 512
        || name.split('/').any(|s| {
            s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-:".contains(&b))
        })
    {
        return Err(io("invalid account record name"));
    }
    Ok(format!("data/{name}"))
}

fn aad(did: &str, key: &str) -> Vec<u8> {
    format!("tinline/account-record/v1\0{did}\0{key}").into_bytes()
}

fn seal(dek: &Dek, did: &str, key: &str, plain: &[u8]) -> Result<Vec<u8>, Error> {
    if plain.len() > MAX_RECORD {
        return Err(io("account record too large"));
    }
    let cipher = Aes256Gcm::new_from_slice(&dek[..]).map_err(io)?;
    let mut nonce = [0; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: &aad(did, key),
            },
        )
        .map_err(io)?;
    Ok([nonce.as_slice(), ct.as_slice()].concat())
}

fn unseal(dek: &Dek, did: &str, key: &str, ct: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
    if !(28..=MAX_RECORD + 28).contains(&ct.len()) {
        return Err(io("invalid sealed record size"));
    }
    let cipher = Aes256Gcm::new_from_slice(&dek[..]).map_err(io)?;
    cipher
        .decrypt(
            Nonce::from_slice(&ct[..12]),
            Payload {
                msg: &ct[12..],
                aad: &aad(did, key),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| io("account record authentication failed"))
}

impl Accounts {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            dir,
            state: Mutex::new(State::default()),
        })
    }

    /// Only DIDs are available while locked. Account and device labels remain encrypted.
    pub fn accounts(&self) -> Result<Vec<String>, Error> {
        let mut result = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().is_some_and(|s| s == "redb") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let did = format!("did:key:{stem}");
                    if identity::public_key_from_did(&did).is_some() {
                        result.push(did);
                    }
                }
            }
        }
        result.sort();
        Ok(result)
    }

    pub fn create(
        &self,
        name: &str,
        device_label: &str,
        pass: &str,
    ) -> Result<(AccountSession, String), Error> {
        let (_, mnemonic) = identity::generate();
        let phrase = mnemonic.to_string();
        let session = self.import(&phrase, name, device_label, pass)?;
        Ok((session, phrase))
    }

    /// Same phrase => same DID; each new enrollment gets a fresh random device key and DEK.
    /// Never replaces an existing account. Initial records commit in one database transaction.
    pub fn import(
        &self,
        phrase: &str,
        name: &str,
        device_label: &str,
        pass: &str,
    ) -> Result<AccountSession, Error> {
        vault::check_passphrase(pass)?;
        for label in [name, device_label] {
            if label.trim().is_empty()
                || label.chars().count() > 64
                || label.chars().any(char::is_control)
            {
                return Err(io(
                    "account/device labels must be 1–64 characters without control characters",
                ));
            }
        }
        let id = identity::recover(phrase).map_err(|_| Error::BadPhrase)?;
        let did = id.did().to_string();
        let secrets = Secrets {
            mnemonic: phrase.to_string(),
            device_secret: proto::new_device_secret(),
        };
        let (vault, dek) = vault::seal(&secrets, Some(pass))?;
        let metadata = Metadata {
            name: name.into(),
            device_label: device_label.into(),
        };
        let plain = Zeroizing::new(serde_json::to_vec(&metadata)?);
        let sealed = seal(&dek, &did, IDENTITY, &plain)?;
        let mut state = self.state.lock();
        let path = self.dir.join(filename(&did)?);
        // Exclusive reservation prevents accidental overwrite by another manager/process.
        let file = {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&path)?
        };
        drop(file);
        let initialized = (|| {
            let store = storage::Store::open(&path).map_err(io)?;
            store
                .apply(&[
                    storage::Op::Put(
                        HEADER.into(),
                        serde_json::to_vec(&Envelope { version: 1, vault })?,
                    ),
                    storage::Op::Put(IDENTITY.into(), sealed),
                ])
                .map_err(io)?;
            Ok::<_, Error>(store)
        })();
        let store = match initialized {
            Ok(store) => store,
            Err(error) => {
                // We own this new reservation. Never remove an existing account on failure.
                let _ = std::fs::remove_file(&path);
                return Err(error);
            }
        };
        // A process kill before initialization commits can still leave an incomplete DB.
        // Recovery/migration must surface it, not silently treat it as an empty account.
        Self::activate(&mut state, did, store, dek, metadata, secrets.device_secret)
    }

    pub fn unlock(&self, did: &str, pass: &str) -> Result<AccountSession, Error> {
        let mut state = self.state.lock();
        if state.active.as_ref().is_some_and(|a| a.session.did == did) {
            return Err(io("account already unlocked"));
        }
        let path = self.dir.join(filename(did)?);
        if !path.is_file() {
            return Err(io("account not found"));
        }
        let store = storage::Store::open(path).map_err(io)?;
        let header = store
            .get(HEADER)
            .map_err(io)?
            .ok_or_else(|| io("missing account envelope"))?;
        let header: Envelope = serde_json::from_slice(&header)?;
        if header.version != 1 {
            return Err(io("unsupported account envelope"));
        }
        let (secrets, dek) = vault::open(&header.vault, pass)?;
        let id = identity::recover(&secrets.mnemonic).map_err(|_| Error::BadPhrase)?;
        if id.did() != did {
            return Err(io("account identity mismatch"));
        }
        let ct = store
            .get(IDENTITY)
            .map_err(io)?
            .ok_or_else(|| io("missing account metadata"))?;
        let metadata = serde_json::from_slice(&unseal(&dek, did, IDENTITY, &ct)?)?;
        Self::activate(
            &mut state,
            did.into(),
            store,
            dek,
            metadata,
            secrets.device_secret,
        )
    }

    fn activate(
        state: &mut State,
        did: String,
        store: storage::Store,
        dek: Dek,
        metadata: Metadata,
        device_secret: [u8; 32],
    ) -> Result<AccountSession, Error> {
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| io("session generation exhausted"))?;
        let session = AccountSession {
            did: did.clone(),
            generation: state.generation,
        };
        state.active = Some(Active {
            session: session.clone(),
            store,
            dek,
            info: AccountInfo {
                did,
                name: metadata.name,
                device_label: metadata.device_label,
                device_public: proto::device_public(&device_secret),
            },
        });
        Ok(session)
    }

    pub fn lock(&self) {
        self.state.lock().active = None;
    }
    pub fn current(&self) -> Option<AccountInfo> {
        self.state.lock().active.as_ref().map(|a| a.info.clone())
    }

    pub fn put(&self, session: &AccountSession, name: &str, bytes: &[u8]) -> Result<(), Error> {
        let key = record_key(name)?;
        let state = self.state.lock();
        let active = state
            .active
            .as_ref()
            .filter(|a| a.session == *session)
            .ok_or(Error::Locked)?;
        let ct = seal(&active.dek, &session.did, &key, bytes)?;
        active.store.put(&key, &ct).map_err(io)
    }

    pub fn get(
        &self,
        session: &AccountSession,
        name: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, Error> {
        let key = record_key(name)?;
        let state = self.state.lock();
        let active = state
            .active
            .as_ref()
            .filter(|a| a.session == *session)
            .ok_or(Error::Locked)?;
        active
            .store
            .get(&key)
            .map_err(io)?
            .map(|ct| unseal(&active.dek, &session.did, &key, &ct))
            .transpose()
    }
}
