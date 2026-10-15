//! Join evidence bridge for the group-call owner. NOT installed into Node's
//! live event sink. It consumes originals that UI-facing GcallEvent currently
//! drops. Current role/device/source locks must be frozen by the caller.
use super::{
    calls::{CallEvent, CallEventKind},
    evidence::Evidence,
    store::{Committed, Ledger},
    *,
};
use crate::gcall::CallAuthority;
use proto::gcall::{self as w, SignedJoin, SignedRoster, SignedStart};

pub(crate) struct JoinProofs {
    pub start: SignedStart,
    pub expected_join: SignedJoin,
    pub roster: SignedRoster,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParticipationView {
    pub group: String,
    pub call_id: String,
    pub issuer: String,
    pub device: [u8; 32],
    pub host_generation: u32,
    pub observed_join_at_ms: u64,
    pub receipt_id: String,
}
fn refusal(reason: &str) -> ReceiptError {
    ReceiptError::Policy(reason.into())
}
fn opened(proofs: &JoinProofs, issuer: &str) -> Result<(w::CallStart, w::Roster), ReceiptError> {
    let s = proofs
        .start
        .open()
        .map_err(|_| refusal("invalid original CallStart"))?;
    let j = proofs
        .expected_join
        .open()
        .map_err(|_| refusal("invalid original local Join"))?;
    let r = proofs
        .roster
        .open()
        .map_err(|_| refusal("invalid original Roster"))?;
    if j.did != issuer
        || j.gid != s.gid
        || r.gid != s.gid
        || j.call_id != s.call_id
        || r.call_id != s.call_id
        || j.host_gen != s.host_gen
        || r.host_gen != s.host_gen
        || r.host_did != s.host_ref.holder_did
        || !r.entries.iter().any(|e| {
            e.did == issuer
                && e.issued_ms == j.issued_ms
                && e.nonce == j.nonce
                && e.sig[..] == proofs.expected_join.sig()[..]
        })
    {
        return Err(refusal(
            "join proof scope/generation/expected Join mismatch",
        ));
    }
    Ok((s, r))
}
/// Called ONLY for a confirmed actual local Joined transition, with that
/// device's original outbound Join. A matching signed roster alone is not proof
/// this device's live session transitioned. The owner's hook must enforce this.
pub(crate) fn persist_join(
    ledger: &Ledger,
    account: &identity::Identity,
    device: [u8; 32],
    observed_at_ms: u64,
    metadata: Vec<(u64, i32)>,
    proofs: &JoinProofs,
    auth: &dyn CallAuthority,
    admit_local: impl FnOnce(&[super::doc::Entry]) -> Result<(), String>,
) -> Result<Committed, ReceiptError> {
    let (claim, evidence) = prepare_join(account, device, observed_at_ms, metadata, proofs, auth)?;
    ledger.write_with_evidence(&claim, account, &evidence, admit_local)
}

/// Freeze the exact body/originals before a durable controller queues a transition.
/// Caller still supplies the actual transition witness and frozen local context.
pub(super) fn prepare_join(
    account: &identity::Identity,
    device: [u8; 32],
    observed_at_ms: u64,
    metadata: Vec<(u64, i32)>,
    proofs: &JoinProofs,
    auth: &dyn CallAuthority,
) -> Result<(Receipt, Vec<Evidence>), ReceiptError> {
    let (s, r) = opened(proofs, account.did())?;
    if !auth.host_permit_ok(&s.gid, &s.host_ref)
        || !auth.is_member(&s.gid, account.did())
        || !auth.is_member(&s.gid, &s.starter_did)
        || r.entries.iter().any(|e| !auth.is_member(&s.gid, &e.did))
    {
        return Err(refusal("current call role/member authority refused"));
    }
    let evidence = vec![
        Evidence::from_blob(&proofs.start)?,
        Evidence::from_blob(&proofs.expected_join)?,
        Evidence::from_blob(&proofs.roster)?,
    ];
    let mut ids: Vec<_> = evidence.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    let at = i64::try_from(observed_at_ms).map_err(|_| ReceiptError::Bounds)?;
    let claim = Receipt {
        scope: Scope {
            conversation: Conversation::Group(s.gid),
            day: day_of(at),
        },
        kind: Kind::Call,
        recipient: account.did().into(),
        device,
        messages: vec![],
        metadata,
        call: Some(CallEvent {
            call_id: s.call_id,
            host_generation: Some(s.host_gen),
            event: CallEventKind::Joined,
            observed_at_ms,
            active_ms: None,
            evidence: ids,
        }),
    };
    claim.check_shape()?;
    Ok((claim, evidence))
}
/// Proof-backed observations, NOT another call-log backend or an all-member
/// roster/attendance/duration count. Consumes only locally accepted originals;
/// historical views do not reapply current host authority and erase observations.
pub(crate) fn joined_views(ledger: &Ledger) -> Result<Vec<ParticipationView>, ReceiptError> {
    let mut views = Vec::new();
    for entry in ledger.entries()? {
        if entry
            .receipt
            .call
            .as_ref()
            .is_some_and(|c| c.event == CallEventKind::Joined)
        {
            views.push(verify_joined_entry(&entry, |id| ledger.evidence(id))?);
        }
    }
    views.sort_by(|a, b| {
        a.observed_join_at_ms
            .cmp(&b.observed_join_at_ms)
            .then_with(|| a.receipt_id.cmp(&b.receipt_id))
    });
    Ok(views)
}
/// Supporting protocol correspondence for an already signature/structure-checked
/// native entry. Caller still decides current/historical authority and device
/// admission; a remote signature is a reported observation, not our own lifecycle.
pub(crate) fn verify_joined_entry(
    entry: &super::doc::Entry,
    mut resolve: impl FnMut(&[u8; 32]) -> Result<Option<Evidence>, ReceiptError>,
) -> Result<ParticipationView, ReceiptError> {
    let c = &entry.receipt;
    let call = c.call.as_ref().ok_or(ReceiptError::Bounds)?;
    if c.kind != Kind::Call || call.event != CallEventKind::Joined {
        return Err(ReceiptError::Bounds);
    }
    if call.evidence.len() != 3 {
        return Err(refusal("joined observation lacks exact evidence set"));
    }
    let mut start = None;
    let mut join = None;
    let mut roster = None;
    for id in &call.evidence {
        let proof = resolve(id)?.ok_or_else(|| refusal("joined original evidence missing"))?;
        if proof.id != *id {
            return Err(refusal("joined evidence identity mismatch"));
        }
        // Each statement type has its own domain, so exactly one of these opens.
        if let Ok(blob) = proof.blob::<w::CallStart>()
            && blob.open().is_ok()
        {
            if start.replace(blob).is_some() {
                return Err(refusal("duplicate start evidence"));
            }
        } else if let Ok(blob) = proof.blob::<w::Join>()
            && blob.open().is_ok()
        {
            if join.replace(blob).is_some() {
                return Err(refusal("duplicate Join evidence"));
            }
        } else if let Ok(blob) = proof.blob::<w::Roster>()
            && blob.open().is_ok()
        {
            if roster.replace(blob).is_some() {
                return Err(refusal("duplicate Roster evidence"));
            }
        } else {
            return Err(refusal("unsupported joined evidence"));
        }
    }
    let proofs = JoinProofs {
        start: start.ok_or_else(|| refusal("start missing"))?,
        expected_join: join.ok_or_else(|| refusal("Join missing"))?,
        roster: roster.ok_or_else(|| refusal("Roster missing"))?,
    };
    let (s, _) = opened(&proofs, &c.recipient)?;
    if c.scope.conversation != Conversation::Group(s.gid.clone())
        || call.call_id != s.call_id
        || call.host_generation != Some(s.host_gen)
        || call.active_ms.is_some()
    {
        return Err(refusal("joined receipt versus originals mismatch"));
    }
    Ok(ParticipationView {
        group: s.gid,
        call_id: s.call_id,
        issuer: c.recipient.clone(),
        device: c.device,
        host_generation: s.host_gen,
        observed_join_at_ms: call.observed_at_ms,
        receipt_id: entry.id.clone(),
    })
}
#[cfg(test)]
pub(crate) mod tests;
