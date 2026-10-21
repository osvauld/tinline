//! The group endpoint key and its rotation state (`ep/*` rows; `docs/rotation.md`).
//!
//! A rotation writes in two atomic steps: `ep_stage` puts the next key in `ep/pending`, and
//! `ep_promote` makes it current together with `made`, `announce_until` and `jitter`. A crash
//! between the two leaves the old key current; `ep_discard_pending` (on start) drops the staged
//! one, so a key that never served never becomes current.

use storage::Op;
use zeroize::Zeroizing;

use super::{GroupStore, io};
use crate::Error;
use crate::rotation::RotationPolicy;

const SECRET: &str = "ep/secret";
const MADE: &str = "ep/made";
const ANNOUNCE: &str = "ep/announce";
const JITTER: &str = "ep/jitter";
const PENDING: &str = "ep/pending";
const POLICY: &str = "ep/policy";

/// The current group endpoint key and its schedule inputs.
pub struct EpState {
    pub secret: Zeroizing<[u8; 32]>,
    /// When this key became current (unix ms).
    pub made: u64,
    /// Until when we tell offline members we moved (unix ms; 0 = never rotated).
    pub announce_until: u64,
    /// The random extra delay drawn for this key, ms.
    pub jitter: u64,
}

fn key(b: Option<Vec<u8>>, row: &str) -> Result<Option<Zeroizing<[u8; 32]>>, Error> {
    let Some(b) = b.map(Zeroizing::new) else {
        return Ok(None);
    };
    let k = <[u8; 32]>::try_from(b.as_slice())
        .map_err(|_| Error::Io(format!("malformed {row} row")))?;
    Ok(Some(Zeroizing::new(k)))
}

fn time(b: Option<Vec<u8>>, row: &str) -> Result<Option<u64>, Error> {
    b.map(|b| {
        <[u8; 8]>::try_from(b.as_slice())
            .map(u64::from_be_bytes)
            .map_err(|_| Error::Io(format!("malformed {row} row")))
    })
    .transpose()
}

impl GroupStore {
    /// The current endpoint state, `None` before the first key. A malformed or partial row is
    /// an error: never a silent new key, never a silent `made = 0`.
    pub fn ep_state(&self) -> Result<Option<EpState>, Error> {
        let get = |row: &str| self.db.get(row).map_err(io);
        let Some(secret) = key(get(SECRET)?, SECRET)? else {
            return Ok(None);
        };
        let made = time(get(MADE)?, MADE)?.ok_or_else(|| Error::Io("ep/made missing".into()))?;
        Ok(Some(EpState {
            secret,
            made,
            announce_until: time(get(ANNOUNCE)?, ANNOUNCE)?.unwrap_or(0),
            jitter: time(get(JITTER)?, JITTER)?.unwrap_or(0),
        }))
    }

    /// The current state, making the first key (current from `now`) if there is none.
    pub fn ep_state_or_init(&self, now: u64, jitter: u64) -> Result<EpState, Error> {
        if let Some(s) = self.ep_state()? {
            return Ok(s);
        }
        let secret = Zeroizing::new(proto::new_device_secret());
        self.apply_append(&[
            self.db.put_op(SECRET, secret.as_slice()).map_err(io)?,
            self.db.put_op(MADE, &now.to_be_bytes()).map_err(io)?,
            self.db.put_op(JITTER, &jitter.to_be_bytes()).map_err(io)?,
        ])?;
        Ok(EpState {
            secret,
            made: now,
            announce_until: 0,
            jitter,
        })
    }

    /// Step 1 of a rotation: a fresh key in `ep/pending` (not current yet); returns it.
    pub fn ep_stage(&self) -> Result<Zeroizing<[u8; 32]>, Error> {
        let next = Zeroizing::new(proto::new_device_secret());
        self.apply_append(&[self.db.put_op(PENDING, next.as_slice()).map_err(io)?])?;
        Ok(next)
    }

    /// Step 2: the staged key becomes current, with its schedule, in one transaction.
    pub fn ep_promote(
        &self,
        now: u64,
        announce_until: u64,
        jitter: u64,
    ) -> Result<(), Error> {
        let pending = key(self.db.get(PENDING).map_err(io)?, PENDING)?
            .ok_or_else(|| Error::Io("no staged endpoint key".into()))?;
        self.apply_append(&[
            self.db.put_op(SECRET, pending.as_slice()).map_err(io)?,
            self.db.put_op(MADE, &now.to_be_bytes()).map_err(io)?,
            self.db
                .put_op(ANNOUNCE, &announce_until.to_be_bytes())
                .map_err(io)?,
            self.db.put_op(JITTER, &jitter.to_be_bytes()).map_err(io)?,
            Op::Delete(PENDING.into()),
        ])
    }

    /// Drops a staged key that was never promoted (on start, or after a failed rebind).
    pub fn ep_discard_pending(&self) -> Result<(), Error> {
        self.apply_append(&[Op::Delete(PENDING.into())])
    }

    /// This device's rotation policy (`RotationPolicy::DEFAULT` until one is set). A stored
    /// policy that does not parse or is out of bounds is an error.
    pub fn rotation_policy(&self) -> Result<RotationPolicy, Error> {
        let Some(b) = self.db.get(POLICY).map_err(io)? else {
            return Ok(RotationPolicy::DEFAULT);
        };
        let p: RotationPolicy =
            serde_json::from_slice(&b).map_err(|_| Error::Io(format!("malformed {POLICY} row")))?;
        match p.invalid() {
            Some(why) => Err(Error::Io(format!("{POLICY}: {why}"))),
            None => Ok(p),
        }
    }

    pub fn set_rotation_policy(&self, p: &RotationPolicy) -> Result<(), Error> {
        if let Some(why) = p.invalid() {
            return Err(Error::Rejected(why.into()));
        }
        self.apply_append(&[self.db.put_op(POLICY, &super::json(p)).map_err(io)?])
    }

    #[cfg(any(test, feature = "testkit"))]
    pub fn ep_pending_for_test(&self) -> Option<[u8; 32]> {
        key(self.db.get(PENDING).unwrap(), PENDING)
            .unwrap()
            .map(|k| *k)
    }

    #[cfg(any(test, feature = "testkit"))]
    pub fn ep_put_raw_for_test(&self, row: &str, value: &[u8]) {
        self.db.put(row, value).unwrap();
    }
}

#[cfg(test)]
mod tests;
