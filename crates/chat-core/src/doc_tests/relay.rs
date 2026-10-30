//! A contact carries every writer's ops of a day, our own from our other devices too
//! (docs/chat.md section 10). Account 0 is Bilal with a phone (install 1) and a laptop
//! (install 2); account 1 is Asha (install 3).

use super::*;

const DAY_MS: i64 = 86_400_000;

/// A batch from the contact `signer`, received by `me`: it may carry `me`'s own messages.
fn contact(signer: usize, me: usize, live: bool) -> Ctx {
    Ctx { signer_did: did(signer), now_ms: NOON, live, history: Some(did(me)) }
}

/// The phone says "Dinner Friday?", Asha answers on top of it; returns Asha's shard.
fn asha_answers_the_phone() -> Shard {
    let phone = shard(0, 1);
    let mut asha = shard(1, 3);
    asha.apply_remote(&phone.add_message(&msg("m1", &did(0), NOON, "Dinner Friday?"), &w(0, 1)).unwrap(), &live(0)).unwrap();
    asha.add_message(&msg("r1", &did(1), NOON + 1, "Yes!"), &w(1, 3)).unwrap();
    asha
}

// KIND: invariant
// The laptop was away while the phone talked. Asha's own ops alone build on the phone's, which
// the laptop lacks (Pending); with every writer's ops it takes the day live.
#[test]
fn a_contact_carries_our_other_devices_messages_to_us() {
    let asha = asha_answers_the_phone();
    let mut laptop = shard(0, 2);
    let own_only = asha.export_own_since(&laptop.vv(), &did(1)).unwrap();
    assert_eq!(laptop.apply_remote(&own_only, &contact(1, 0, true)), Err(Reject::Pending));
    let all = asha.export_all_since(&laptop.vv()).unwrap();
    let r = laptop.apply_remote(&all, &contact(1, 0, true)).unwrap();
    assert_eq!(r.added, vec!["m1".to_string(), "r1".to_string()]);
    assert_eq!(laptop.messages().unwrap(), asha.messages().unwrap());
}

// KIND: invariant
// What a contact carries of ours is proven by our DID signature: a message in our name signed
// by the contact, under our writer ID or its own, is refused.
#[test]
fn a_contact_cannot_put_words_in_our_mouth() {
    let asha = asha_answers_the_phone();
    // The laptop holds the day already, so each forgery is judged on its own.
    let laptop = || {
        let mut l = shard(0, 2);
        l.apply_remote(&asha.export_all_since(&l.vv()).unwrap(), &contact(1, 0, true)).unwrap();
        l
    };
    let u = span(&asha, wid(0, 1), &rec_of("f1", 0, 1, NOON + 2, "I owe you"), by(1));
    assert_eq!(laptop().apply_remote(&u, &contact(1, 0, true)), Err(Reject::BadSignature));
    let u = span(&asha, wid(1, 3), &rec_of("f1", 0, 3, NOON + 2, "I owe you"), by(1));
    assert!(laptop().apply_remote(&u, &contact(1, 0, true)).is_err());
    // Only the conversation's two DIDs: a stranger's message is refused even with its own signature.
    let u = span(&asha, wid(2, 5), &rec_of("s1", 2, 5, NOON + 2, "stranger"), by(2));
    assert_eq!(laptop().apply_remote(&u, &contact(1, 0, false)), Err(Reject::WrongAuthor));
    let l = laptop();
    assert_eq!(l.messages().unwrap(), asha.messages().unwrap(), "nothing was taken");
}

// KIND: invariant
// The live window holds for what the signer writes, not for our own messages it carries: our
// month-old message arrives live, a month-old message of the contact's does not.
#[test]
fn only_the_signers_new_messages_are_held_to_the_live_window() {
    let asha = asha_answers_the_phone();
    let later = Ctx { now_ms: NOON + 30 * DAY_MS, ..contact(1, 0, true) };
    let mut laptop = shard(0, 2);
    assert_eq!(laptop.apply_remote(&asha.export_all_since(&laptop.vv()).unwrap(), &later), Err(Reject::OutOfRange));
    // A day where Asha holds only our m1: carried live a month later, accepted.
    let phone_only = shard(0, 1);
    let m1 = phone_only.add_message(&msg("m1", &did(0), NOON, "Dinner Friday?"), &w(0, 1)).unwrap();
    let mut asha2 = shard(1, 3);
    asha2.apply_remote(&m1, &live(0)).unwrap();
    let mut laptop = shard(0, 2);
    assert_eq!(laptop.apply_remote(&asha2.export_all_since(&laptop.vv()).unwrap(), &later).unwrap().added, vec!["m1".to_string()]);
}
