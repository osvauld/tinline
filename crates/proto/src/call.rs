//! Call hello and acceptance.

use super::*;

// ---- calls -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub did: String,
    pub device: [u8; 32],
    pub grant_id: String,
    /// The verified token it presented (any capability: see `accept_hello`).
    pub grant: Grant,
}

/// `_me` is unused today (the attestation and grant are already signed); kept so the call
/// site mirrors `accept_call_hello` and a future per-call signature needs no API break.
pub fn call_hello(
    _me: &Identity,
    my_device_attestation: SignedAttestation,
    grant_from_them: SignedGrant,
    call_id: String,
) -> Msg {
    Msg::CallHello {
        call_id,
        attestation: my_device_attestation,
        grant: grant_from_them,
        relay: None,
        devices: None,
        video: None,
    }
}

/// `accept_hello` that also requires the presented token to carry `Cap::Call`.
pub fn accept_call_hello(
    me: &Identity,
    msg: &Msg,
    remote_device: [u8; 32],
    now: u64,
    revoked_grant_ids: &HashSet<String>,
    is_contact: impl Fn(&str) -> bool,
) -> Result<Caller> {
    let caller = accept_hello(me, msg, remote_device, now, revoked_grant_ids, is_contact)?;
    if !caller.grant.allows(Cap::Call) {
        return Err(Error::BadCapability);
    }
    Ok(caller)
}

/// Who is at the other end of a `CallHello` or `GroupLetter` (or a chat `Auth`, which carries the
/// same proof):
/// attested device = the connection's, a token we issued to that DID (any capability, unexpired,
/// not revoked by id), and a contact. Which capability the token must carry, and the per-DID
/// cut-off, are checked by the caller after its block check, so a blocked DID still gets the
/// block notice rather than a generic refusal.
pub fn accept_hello(
    me: &Identity,
    msg: &Msg,
    remote_device: [u8; 32],
    now: u64,
    revoked_grant_ids: &HashSet<String>,
    is_contact: impl Fn(&str) -> bool,
) -> Result<Caller> {
    let (attestation, grant) = match msg {
        Msg::CallHello {
            attestation, grant, ..
        } => (attestation, grant),
        Msg::GroupLetter {
            attestation, grant, ..
        } => (attestation, grant),
        _ => return Err(Error::UnexpectedMessage),
    };
    let att = verify_attestation(attestation)?;
    if att.device != remote_device {
        return Err(Error::DeviceMismatch);
    }
    // Holder is the attested DID, so a grant lifted by someone else fails here.
    let g = verify_grant(grant, me.did(), &att.did, now, revoked_grant_ids)?;
    if !is_contact(&att.did) {
        return Err(Error::NotContact);
    }
    Ok(Caller {
        did: att.did,
        device: remote_device,
        grant_id: g.id.clone(),
        grant: g,
    })
}
