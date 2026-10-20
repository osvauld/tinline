//! Direct group invitations (G3) as this device stores them (`inv/{id}`). Delivery, answers and
//! the UniFFI view are p2pcore's `group/invitations.rs`.

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::store::{GroupStore, io, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupInvitationState {
    /// Not answered yet.
    Pending,
    /// The invitee joined the group.
    Joined,
    Declined,
    /// The invite ran out (7 days) before it was answered.
    Expired,
    /// The invitee blocks the inviter (P5): it was never shown to them.
    Blocked,
}

/// One invitation as this device holds it (group store `inv/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvRow {
    pub id: String,
    pub group: String,
    pub group_name: String,
    pub inviter: String,
    pub inviter_name: String,
    pub invitee: String,
    pub invitee_name: String,
    pub outgoing: bool,
    pub state: GroupInvitationState,
    pub at: u64,
    pub expires: u64,
    /// `codec` bytes of an `InviteToken` (with the signed invite).
    pub token: Vec<u8>,
    #[serde(default)]
    pub delivered: bool,
    /// This device still has to deliver this row's letter (the invite, or our decline).
    #[serde(default)]
    pub owe: bool,
}

impl GroupStore {
    pub fn put_inv(&self, r: &InvRow) -> Result<(), Error> {
        self.db.put(&format!("inv/{}", r.id), &json(r)).map_err(io)
    }

    pub fn inv(&self, id: &str) -> Result<Option<InvRow>, Error> {
        Ok(self.db.get(&format!("inv/{id}")).map_err(io)?.and_then(|v| serde_json::from_slice(&v).ok()))
    }

    pub fn invs(&self) -> Result<Vec<InvRow>, Error> {
        Ok(self.db.scan("inv/").map_err(io)?.into_iter().filter_map(|(_, v)| serde_json::from_slice(&v).ok()).collect())
    }

    pub fn drop_inv(&self, id: &str) -> Result<(), Error> {
        self.db.apply(&[storage::Op::Delete(format!("inv/{id}"))]).map_err(io)
    }
}

