//! Call hello and acceptance.

use super::*;

// ---- calls -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub did: String,
    pub device: [u8; 32],
    /// The verified token it presented (any capability: see `accept_hello`).
    pub grant: Grant,
}

/// A call (or chat `Auth`) hello: the attestation and the token are already signed.
pub fn call_hello(
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
        camera: false,
    }
}

/// `accept_hello` that also requires the presented token to carry `Cap::Call`.
pub fn accept_call_hello(
    me: &Identity,
    msg: &Msg,
    remote_device: [u8; 32],
    now_ms: u64,
    is_contact: impl Fn(&str) -> bool,
) -> Result<Caller> {
    let caller = accept_hello(me, msg, remote_device, now_ms, is_contact)?;
    if !caller.grant.allows(Cap::Call) {
        return Err(Error::BadCapability);
    }
    Ok(caller)
}

/// Who is at the other end of a `CallHello` or `GroupLetter` (or a chat `Auth`, which carries the
/// same proof):
/// attested device = the connection's, a token we issued to that DID (any capability,
/// `redeem`ed without our store), and a contact. Which capability the token must carry, and the
/// per-DID cut-off (`Issued::admit` under our store), are checked by the caller after its block
/// check, so a blocked DID still gets the block notice rather than a generic refusal.
pub fn accept_hello(
    me: &Identity,
    msg: &Msg,
    remote_device: [u8; 32],
    now_ms: u64,
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
    let g = grant.redeem(&Expect { iss: Some(me.did()), holder: &att.did, now_ms }, &Unrecorded)?;
    if !is_contact(&att.did) {
        return Err(Error::NotContact);
    }
    Ok(Caller {
        did: att.did,
        device: remote_device,
        grant: g,
    })
}
