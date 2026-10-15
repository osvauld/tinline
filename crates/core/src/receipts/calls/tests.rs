use super::*;
use crate::{
    chat::doc::writer_id,
    receipts::{doc::ReceiptDoc, tests::sample},
};
pub(crate) fn claim(a: &identity::Identity, group: bool) -> Receipt {
    let mut c = sample(a, Kind::Delivered, group);
    c.kind = Kind::Call;
    c.messages.clear();
    c.call = Some(CallEvent {
        call_id: proto::gcall::new_call_id(),
        host_generation: group.then_some(1),
        event: CallEventKind::Joined,
        observed_at_ms: 0,
        active_ms: None,
        evidence: vec![[1; 32]],
    });
    c
}
#[test]
fn individual_and_group_call_observations_use_same_native_receipt_pipeline() {
    let a = identity::generate().0;
    for group in [false, true] {
        let c = claim(&a, group);
        let mut source = ReceiptDoc::new(c.scope.clone(), writer_id(a.did(), &c.device)).unwrap();
        let (entry, update) = source.write(&c, &a, |_| Ok(())).unwrap().unwrap();
        let mut target = ReceiptDoc::new(c.scope.clone(), 9).unwrap();
        target.apply_remote(&update, |_| Ok(())).unwrap();
        assert_eq!(target.entries().unwrap(), vec![entry.clone()]);
        assert_eq!(target.raw(&entry.id), source.raw(&entry.id));
        assert_eq!(
            open_receipt(&target.raw(&entry.id).unwrap(), &c.scope).unwrap(),
            c
        );
    }
}
#[test]
fn all_typed_session_observation_fields_are_signature_bound() {
    let a = identity::generate().0;
    let base = claim(&a, true);
    let signed = base.sign_claim(&a).unwrap();
    for field in 0..6 {
        let mut c = base.clone();
        let event = c.call.as_mut().unwrap();
        match field {
            0 => event.call_id = proto::gcall::new_call_id(),
            1 => event.host_generation = Some(2),
            2 => event.event = CallEventKind::ObservedHostEnd,
            3 => event.observed_at_ms = 1,
            4 => event.evidence.push([2; 32]),
            _ => {
                event.event = CallEventKind::ParticipationEnded {
                    outcome: CallOutcome::HostLost,
                };
                event.active_ms = Some(1000);
            }
        }
        let bytes =
            SignedReceipt::from_parts(cryptography::codec::to_bytes(&c).unwrap(), *signed.sig())
                .to_bytes();
        assert_eq!(open_receipt(&bytes, &c.scope), Err(ReceiptError::Signature));
    }
}
#[test]
fn mixed_message_call_payloads_wrong_day_generation_and_evidence_shape_refused() {
    let a = identity::generate().0;
    for field in 0..10 {
        let mut c = claim(&a, true);
        match field {
            0 => c.messages.push([1; 32]),
            1 => c.kind = Kind::Read,
            2 => c.call = None,
            3 => c.call.as_mut().unwrap().host_generation = Some(0),
            4 => c.call.as_mut().unwrap().observed_at_ms = crate::chat::doc::DAY_MS as u64,
            5 => c.call.as_mut().unwrap().evidence.clear(),
            6 => c.call.as_mut().unwrap().evidence.push([1; 32]),
            7 => c.call.as_mut().unwrap().active_ms = Some(1000),
            8 => c.call.as_mut().unwrap().call_id = "x".repeat(MAX_CALL_ID_BYTES + 1),
            _ => c.call.as_mut().unwrap().observed_at_ms = u64::MAX,
        }
        assert_eq!(c.sign_claim(&a), Err(ReceiptError::Bounds));
    }
    let mut dm = claim(&a, false);
    dm.call.as_mut().unwrap().host_generation = Some(1);
    assert_eq!(dm.sign_claim(&a), Err(ReceiptError::Bounds));
}
#[test]
fn interruption_and_local_ending_are_not_host_end_or_global_duration() {
    let a = identity::generate().0;
    let mut c = claim(&a, true);
    for event in [
        CallEventKind::Interrupted,
        CallEventKind::ParticipationEnded {
            outcome: CallOutcome::Left,
        },
        CallEventKind::ParticipationEnded {
            outcome: CallOutcome::AnsweredElsewhere,
        },
    ] {
        c.call.as_mut().unwrap().event = event;
        c.call.as_mut().unwrap().active_ms = Some(1234);
        let bytes = c.sign_claim(&a).unwrap().to_bytes();
        assert_eq!(open_receipt(&bytes, &c.scope).unwrap(), c);
    }
    c.call.as_mut().unwrap().event = CallEventKind::ObservedHostEnd;
    assert_eq!(c.sign_claim(&a), Err(ReceiptError::Bounds));
}
#[test]
fn signed_join_claim_does_not_replace_caller_confirmation_admission() {
    let a = identity::generate().0;
    let c = claim(&a, true);
    let mut d = ReceiptDoc::new(c.scope.clone(), writer_id(a.did(), &c.device)).unwrap();
    assert!(
        d.write(&c, &a, |_| Err(
            "only a Join request, no confirmed local admission".into()
        ))
        .is_err()
    );
    assert!(d.vv().is_empty());
}
