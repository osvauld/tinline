//! U3 chat requests as this device stores them: the request rows, the sealed mail a host keeps
//! for a member, and the mail ids already handled. Sealing and the request flow are p2pcore's
//! `group/requests.rs`.

use serde::{Deserialize, Serialize};
use storage::Op;

use crate::Error;
use crate::mail;
use crate::store::{GroupStore, io, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReqState {
    Pending,
    Accepted,
    Declined,
    Blocked,
}

/// One request as this device holds it (group store `req/{id}`; local, PROVISIONAL: not synced
/// to own devices).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqRow {
    pub req: String,
    pub group: String,
    pub peer: String,
    pub peer_name: String,
    pub outgoing: bool,
    pub state: ReqState,
    pub at: u64,
    /// Outgoing, accepted: the ticket to add them from, until added.
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub added: bool,
    #[serde(default)]
    pub next_try: u64,
}

impl GroupStore {
    // ---- requests and mail (U3) --------------------------------------------------------------

    pub fn put_req(&self, r: &ReqRow) -> Result<(), Error> {
        self.db.put(&format!("req/{}", r.req), &json(r)).map_err(io)
    }

    pub fn req(&self, id: &str) -> Result<Option<ReqRow>, Error> {
        Ok(self.db.get(&format!("req/{id}")).map_err(io)?.and_then(|v| serde_json::from_slice(&v).ok()))
    }

    pub fn reqs(&self) -> Result<Vec<ReqRow>, Error> {
        Ok(self.db.scan("req/").map_err(io)?.into_iter().filter_map(|(_, v)| serde_json::from_slice(&v).ok()).collect())
    }

    pub fn put_out(&self, gid: &str, id: &str, mail: &[u8]) -> Result<(), Error> {
        self.db.put(&format!("out/{gid}/{id}"), mail).map_err(io)
    }

    pub fn outbox(&self, gid: &str) -> Result<Vec<Vec<u8>>, Error> {
        Ok(self.db.scan(&format!("out/{gid}/")).map_err(io)?.into_iter().map(|(_, v)| v).collect())
    }

    pub fn drop_out(&self, gid: &str, id: &str) -> Result<(), Error> {
        self.db.apply(&[Op::Delete(format!("out/{gid}/{id}"))]).map_err(io)
    }

    /// Host side: keeps mail for `to` until `expires`; false if it was already here.
    pub fn put_mail(&self, gid: &str, to: &str, id: &str, expires: u64, mail: &[u8]) -> Result<bool, Error> {
        let k = format!("mail/{gid}/{to}/{id}");
        if self.db.get(&k).map_err(io)?.is_some() {
            return Ok(false);
        }
        self.db.put(&k, &cryptography::codec::to_bytes(&(expires, mail.to_vec())).expect("serialises")).map_err(io)?;
        Ok(true)
    }

    /// Host side: unexpired mail of `gid` (for `to`, or all), `(to, bytes)`; expired rows go.
    pub fn mail(&self, gid: &str, to: Option<&str>, now_ms: u64) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let p = match to {
            Some(t) => format!("mail/{gid}/{t}/"),
            None => format!("mail/{gid}/"),
        };
        let mut out = Vec::new();
        let mut dead = Vec::new();
        for (k, v) in self.db.scan(&p).map_err(io)? {
            let Ok((exp, b)) = cryptography::codec::from_bytes::<(u64, Vec<u8>)>(&v, storage::MAX_ROW) else { continue };
            if exp < now_ms {
                dead.push(Op::Delete(k));
                continue;
            }
            let rest = k.strip_prefix(&format!("mail/{gid}/")).unwrap_or_default();
            let to = rest.rsplit_once('/').map(|(t, _)| t.to_string()).unwrap_or_default();
            out.push((to, b));
        }
        if !dead.is_empty() {
            let _ = self.db.apply(&dead);
        }
        Ok(out)
    }

    /// Recipient side: remembers mail `id` as seen (`IssuanceStore::spent` answers for it).
    pub fn mark_seen(&self, id: &[u8; 16]) -> Result<(), Error> {
        self.db.put(&format!("seen/{}", mail::id_hex(id)), &[1]).map_err(io)
    }
}

/// The group DB as an issuer's memory: a nonce is spent once seen (mail ids). Group invites are
/// bearer until they expire (D17): their nonces are never marked, and nothing is cut off.
impl proto::IssuanceStore for GroupStore {
    fn spent(&self, _: &str, nonce: &[u8], _: &str) -> bool {
        self.db.get(&format!("seen/{}", hex::encode(nonce))).ok().flatten().is_some()
    }

    fn cut_off_ms(&self, _: &str, _: cryptography::domains::Domain) -> u64 {
        0
    }
}

