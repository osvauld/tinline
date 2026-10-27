//! Own-device hello and the add-contact handshake.

use super::*;

// ---- own-device sync ---------------------------------------------------------------

/// Authorization for `tinline/self/3`: the peer is another device of *our* DID, it is the
/// device the transport authenticated, and it has not been removed. No grant is involved.
pub fn accept_self_hello(
    attestation: &SignedAttestation,
    my_did: &str,
    remote_device: [u8; 32],
    tombstoned: &HashSet<[u8; 32]>,
) -> Result<Attestation> {
    let att = attestation.open_from(my_did)?;
    let dev = att.device;
    if dev != remote_device {
        return Err(Error::DeviceMismatch);
    }
    if tombstoned.contains(&dev) {
        return Err(Error::Tombstoned);
    }
    Ok(att)
}

// ---- add-contact handshake ---------------------------------------------------------

/// The joiner's half of an add, kept until the owner's `ContactWelcome` arrives: on the same
/// connection (the owner took it at once) or later on a connection the owner dials back after
/// its user accepted the request (`ContactPending`). Serialisable so the joiner can keep it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingContact {
    pub did: String,
    pub name: String,
    pub device: [u8; 32],
    pub nonce: String,
    my_did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewContact {
    pub did: String,
    pub name: String,
    pub device: [u8; 32],
    /// Their message token for us.
    pub grant_from_them: SignedGrant,
    /// Their call token for us, if they let us call.
    pub call_grant_from_them: Option<SignedGrant>,
    pub redeemed_nonce: String,
}

/// The signed invite inside a `ContactHello` (signature checked, nothing else): for the owner
/// to learn which of its tickets a held request used and when that ticket expires.
pub fn hello_invite(msg: &Msg) -> Result<InviteClaim> {
    match msg {
        Msg::ContactHello { invite, .. } => open_invite(invite),
        _ => Err(Error::UnexpectedMessage),
    }
}

/// The entry the owner keeps in its redeemed set (a record) once `joiner` has been added with the
/// ticket whose nonce is `nonce`.
pub fn redeemed_key(joiner: &str, nonce: &str) -> String {
    format!("{nonce}/{joiner}")
}

/// Binds the joiner's DID to *this* invite and *this* pair of devices, so a hello can't be
/// lifted onto another connection or invite: a detached signature over
/// `[nonce, joiner_device, owner_device]` under `domains::BIND`.
pub(crate) fn binding_parts<'a>(
    nonce: &'a [u8],
    joiner_device: &'a [u8; 32],
    owner_device: &'a [u8; 32],
) -> [&'a [u8]; 3] {
    [nonce, joiner_device, owner_device]
}

/// `contact_hello_with` for a ticket contact (message + call).
pub fn contact_hello(
    me: &Identity,
    my_device: [u8; 32],
    ticket: &ContactTicket,
    my_name: &str,
    now_ms: u64,
    grant_ttl_ms: u64,
) -> Result<(Msg, PendingContact)> {
    contact_hello_with(
        me,
        my_device,
        ticket,
        my_name,
        now_ms,
        grant_ttl_ms,
        TokenPolicy::TICKET,
    )
}

/// The joiner's hello, giving the ticket owner the tokens `policy` says. Tickets count in
/// seconds (`now_ms / 1000`), tokens in ms.
pub fn contact_hello_with(
    me: &Identity,
    my_device: [u8; 32],
    ticket: &ContactTicket,
    my_name: &str,
    now_ms: u64,
    grant_ttl_ms: u64,
    policy: TokenPolicy,
) -> Result<(Msg, PendingContact)> {
    let now = now_ms / 1000;
    let claim = ticket.verify(now)?;
    if claim.iss == me.did() {
        return Err(Error::SelfContact);
    }
    let their_device = claim.device;
    let nonce = dec(&claim.nonce)?;
    let binding = enc(signed::sign_detached(
        me,
        domains::BIND,
        &binding_parts(&nonce, &my_device, &their_device),
    ));
    let (message, call) = policy.issue(me, &claim.iss, now_ms, grant_ttl_ms);
    let msg = Msg::ContactHello {
        relay: None,
        attestation: attest(me, my_device, now),
        name: my_name.to_string(),
        invite: ticket.invite.clone(),
        binding,
        grant_for_you: message,
        call_grant_for_you: call,
        wake: None,
    };
    let pending = PendingContact {
        did: claim.iss,
        name: sanitize_name(&claim.name),
        device: their_device,
        nonce: claim.nonce,
        my_did: me.did().to_string(),
    };
    Ok((msg, pending))
}

/// `accept_contact_hello_with`, giving every joiner message + call (ticket default).
pub fn accept_contact_hello(
    me: &Identity,
    my_device: [u8; 32],
    msg: &Msg,
    remote_device: [u8; 32],
    now_ms: u64,
    tickets: &dyn IssuanceStore,
    grant_ttl_ms: u64,
) -> Result<(Msg, NewContact)> {
    accept_contact_hello_with(
        me,
        my_device,
        msg,
        remote_device,
        now_ms,
        tickets,
        grant_ttl_ms,
        |_| TokenPolicy::TICKET,
    )
}

/// The ticket owner's side: checks the hello and answers with the tokens `policy(joiner DID)`
/// says (the caller knows whether that DID is an existing contact it keeps message-only, and
/// its cut-off). `tickets` says whether this joiner may still redeem the ticket's nonce.
#[allow(clippy::too_many_arguments)]
pub fn accept_contact_hello_with(
    me: &Identity,
    my_device: [u8; 32],
    msg: &Msg,
    remote_device: [u8; 32],
    now_ms: u64,
    tickets: &dyn IssuanceStore,
    grant_ttl_ms: u64,
    policy: impl Fn(&str) -> TokenPolicy,
) -> Result<(Msg, NewContact)> {
    let Msg::ContactHello {
        attestation,
        name,
        invite,
        binding,
        grant_for_you,
        call_grant_for_you,
        ..
    } = msg
    else {
        return Err(Error::UnexpectedMessage);
    };
    let now = now_ms / 1000;
    let claim = open_invite(invite)?;
    if claim.iss != me.did() {
        return Err(Error::WrongIssuer);
    }
    if now >= claim.exp {
        return Err(Error::Expired);
    }
    if claim.device != my_device {
        return Err(Error::DeviceMismatch);
    }
    let att = verify_attestation(attestation)?;
    if att.device != remote_device {
        return Err(Error::DeviceMismatch);
    }
    let nonce = dec(&claim.nonce)?;
    if tickets.spent(&claim.iss, &nonce, &att.did) {
        return Err(Error::Replayed);
    }
    let sig: [u8; 64] = dec(binding)?.try_into().map_err(|_| Error::Decode)?;
    signed::verify_detached(
        Signer::Did(&att.did),
        domains::BIND,
        &binding_parts(&nonce, &remote_device, &my_device),
        &sig,
    )?;
    verify_contact_tokens(
        grant_for_you,
        call_grant_for_you.as_ref(),
        &att.did,
        me.did(),
        now_ms,
    )?;

    let (message, call) = policy(&att.did).issue(me, &att.did, now_ms, grant_ttl_ms);
    let welcome = Msg::ContactWelcome {
        attestation: attest(me, my_device, now),
        name: sanitize_name(&claim.name),
        grant_for_you: message,
        call_grant_for_you: call,
        wake: None,
    };
    let contact = NewContact {
        did: att.did,
        name: sanitize_name(name),
        device: remote_device,
        grant_from_them: grant_for_you.clone(),
        call_grant_from_them: call_grant_for_you.clone(),
        redeemed_nonce: claim.nonce,
    };
    Ok((welcome, contact))
}

pub fn accept_contact_welcome(
    me: &Identity,
    pending: &PendingContact,
    msg: &Msg,
    remote_device: [u8; 32],
    now_ms: u64,
) -> Result<NewContact> {
    let Msg::ContactWelcome {
        attestation,
        grant_for_you,
        call_grant_for_you,
        ..
    } = msg
    else {
        return Err(Error::UnexpectedMessage);
    };
    if pending.my_did != me.did() {
        return Err(Error::WrongHolder);
    }
    let att = attestation.open_from(&pending.did)?;
    let dev = att.device;
    if dev != remote_device || dev != pending.device {
        return Err(Error::DeviceMismatch);
    }
    verify_contact_tokens(
        grant_for_you,
        call_grant_for_you.as_ref(),
        &pending.did,
        me.did(),
        now_ms,
    )?;
    Ok(NewContact {
        did: pending.did.clone(),
        // The ticket's signed name, not the welcome's unsigned one.
        name: pending.name.clone(),
        device: dev,
        grant_from_them: grant_for_you.clone(),
        call_grant_from_them: call_grant_for_you.clone(),
        redeemed_nonce: pending.nonce.clone(),
    })
}
