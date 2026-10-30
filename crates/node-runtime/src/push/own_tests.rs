//! Own devices wake each other (`own.rs`, `docs/push.md` "Own devices"): the token a phone
//! issues for each other own device, its intake, the throttle of the wake, the take-back on
//! unlink and what a wake from an own device does. In-process nodes and the fake relay of
//! `tests.rs`. Names start with `wake_own_` (`cargo test -p node-runtime --lib wake_own`).

use std::sync::Weak;

use iroh::address_lookup::memory::MemoryLookup;
use wake::WakeAction;

use super::ring_tests::Kicks;
use super::tests::*;
use super::*;
use crate::hooks::NodeHooks;
use crate::store::OwnDevice;

fn entry(t: &T) -> OwnDevice {
    OwnDevice {
        device: t.me.device,
        attestation: t.me.attestation.clone(),
        label: String::new(),
        removed: false,
        last_seen: 0,
        relay: None,
        links: Vec::new(),
        rotated: false,
    }
}

/// `phone` and `laptop` are two devices of one account, each in the other's registry; the phone
/// has a push target, the laptop none (desktop).
async fn phone_and_laptop(lookup: &MemoryLookup) -> (T, T) {
    let phone = node(lookup).await;
    let laptop = second_device(lookup, &phone).await;
    for t in [&phone, &laptop] {
        let (p, l) = (entry(&phone), entry(&laptop));
        t.edit(|st| st.registry.extend([p, l]));
    }
    set_target(&phone, Some(fcm(7)));
    (phone, laptop)
}

fn issued_for_own(t: &T, other: &T) -> Option<Issued> {
    t.state(|st| {
        st.wake
            .as_ref()?
            .issued_for(&own_key(&other.me.device))
            .cloned()
    })
}

fn held_own(t: &T, other: &T) -> Option<wake::Held> {
    t.state(|st| {
        st.wake
            .as_ref()?
            .held
            .iter()
            .find(|e| e.did == t.did() && e.device == other.me.device)
            .map(|e| e.held.clone())
    })
}

/// The laptop holds the phone's token, as after an own-device session.
async fn laptop_holds_phone_token(phone: &T, laptop: &T) -> String {
    eventually("the phone issued a token for the laptop", || {
        phone.ctx.own_wake_token_for(&laptop.me.device).is_some()
    })
    .await;
    let token = phone.ctx.own_wake_token_for(&laptop.me.device).unwrap();
    assert!(
        laptop
            .ctx
            .own_wake_token_in(phone.me.device, Some(token.clone()))
    );
    token.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_own_a_phone_issues_one_message_token_per_other_own_device() {
    let lookup = MemoryLookup::new();
    let (phone, laptop) = phone_and_laptop(&lookup).await;
    eventually("token for the laptop", || {
        issued_for_own(&phone, &laptop).is_some()
    })
    .await;
    let issued = issued_for_own(&phone, &laptop).unwrap();
    assert_eq!(issued.kinds, Kinds::MSG, "an own device never rings us");
    assert!(
        issued_for_own(&phone, &phone).is_none(),
        "no token for ourselves"
    );
    assert!(
        phone.state(|st| st.wake_outbox.is_empty()),
        "own tokens go over the own-device link, not WakeTokens"
    );
    assert_eq!(
        phone.ctx.own_wake_token_for(&laptop.me.device),
        Some((issued.token, issued.exp_ms))
    );
    // The laptop has no push target: nothing to give.
    assert!(laptop.ctx.own_wake_token_for(&phone.me.device).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_own_a_token_is_kept_only_from_a_live_own_device_and_dropped_on_none() {
    let lookup = MemoryLookup::new();
    let (phone, laptop) = phone_and_laptop(&lookup).await;
    let token = laptop_holds_phone_token(&phone, &laptop).await;
    let held = held_own(&laptop, &phone).unwrap();
    assert_eq!(
        (held.token.as_str(), held.kinds),
        (token.as_str(), Kinds::MSG)
    );
    assert!(laptop.ctx.can_wake_own(&phone.me.device));
    // Not an own device of this account (a stranger's key, or a contact's).
    let stranger = node(&lookup).await;
    assert!(!laptop.ctx.own_wake_token_in(
        stranger.me.device,
        Some((token.clone(), now_ms() + 86_400_000))
    ));
    assert!(!laptop.state(|st| {
        st.wake
            .as_ref()
            .unwrap()
            .held
            .iter()
            .any(|e| e.device == stranger.me.device)
    }));
    // Expired or oversized tokens are refused.
    assert!(
        !laptop
            .ctx
            .own_wake_token_in(phone.me.device, Some(("t".into(), now_ms() - 1)))
    );
    assert!(
        !laptop
            .ctx
            .own_wake_token_in(phone.me.device, Some(("x".repeat(5000), now_ms() + 1000)))
    );
    assert_eq!(
        held_own(&laptop, &phone).unwrap().token,
        token,
        "a refused one leaves the held token alone"
    );
    // The phone turned push off: it says so and the laptop forgets the token.
    assert!(laptop.ctx.own_wake_token_in(phone.me.device, None));
    assert!(held_own(&laptop, &phone).is_none());
    assert!(!laptop.ctx.can_wake_own(&phone.me.device));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_own_an_unreachable_own_device_is_woken_exactly_once_per_throttle() {
    let lookup = MemoryLookup::new();
    let (phone, laptop) = phone_and_laptop(&lookup).await;
    let token = laptop_holds_phone_token(&phone, &laptop).await;
    phone.ep.close().await;
    // The sync dial loop fails every few seconds while the phone sleeps.
    assert!(laptop.ctx.wake_own_device(phone.me.device).await);
    for _ in 0..5 {
        assert!(
            !laptop.ctx.wake_own_device(phone.me.device).await,
            "throttled"
        );
    }
    assert_eq!(
        *laptop.relay.wakes.lock(),
        vec![(token, Kind::Msg)],
        "exactly one content-free msg wake"
    );
    // A session with it starts the schedule over; the 30 s floor stays.
    assert_eq!(held_own(&laptop, &phone).unwrap().msg_wakes, 1);
    laptop.ctx.own_reached(&phone.me.device);
    assert_eq!(held_own(&laptop, &phone).unwrap().msg_wakes, 0);
    assert!(
        !laptop.ctx.wake_own_device(phone.me.device).await,
        "the floor"
    );
    assert_eq!(laptop.relay.wakes.lock().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_own_a_dead_token_is_dropped_and_a_contact_path_cannot_use_it() {
    let lookup = MemoryLookup::new();
    let (phone, laptop) = phone_and_laptop(&lookup).await;
    laptop_holds_phone_token(&phone, &laptop).await;
    // The contact paths never wake an own device (it is not a contact).
    assert!(
        !laptop
            .ctx
            .can_wake(&laptop.did(), &phone.me.device, Kind::Msg)
    );
    assert!(
        !laptop
            .ctx
            .wake_device(&laptop.did(), phone.me.device, Kind::Msg)
            .await
    );
    assert!(laptop.relay.wakes.lock().is_empty());
    laptop.relay.wake_status.store(410, Ordering::SeqCst);
    assert!(!laptop.ctx.wake_own_device(phone.me.device).await);
    assert!(held_own(&laptop, &phone).is_none(), "410 drops the token");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_own_an_unlinked_device_loses_its_token_and_ours_is_revoked() {
    let lookup = MemoryLookup::new();
    let (phone, laptop) = phone_and_laptop(&lookup).await;
    eventually("token for the laptop", || {
        issued_for_own(&phone, &laptop).is_some()
    })
    .await;
    let token = issued_for_own(&phone, &laptop).unwrap().token;
    // The phone also holds a (made-up) token of the laptop's.
    assert!(phone.ctx.own_wake_token_in(
        laptop.me.device,
        Some(("laptop-token".into(), now_ms() + 86_400_000))
    ));
    phone.edit(|st| {
        st.registry
            .iter_mut()
            .filter(|e| e.device == laptop.me.device)
            .for_each(|e| e.removed = true)
    });
    phone.ctx.wake_kick();
    eventually("revoked at the relay", || {
        phone.relay.revokes.lock().contains(&token)
    })
    .await;
    assert!(issued_for_own(&phone, &laptop).is_none());
    assert!(
        held_own(&phone, &laptop).is_none(),
        "a removed device's token is dropped"
    );
    assert!(
        !phone.ctx.own_wake_token_in(
            laptop.me.device,
            Some(("again".into(), now_ms() + 86_400_000))
        ),
        "nor taken again"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_own_a_wake_from_an_own_device_syncs_instead_of_dialling_a_contact() {
    let lookup = MemoryLookup::new();
    let (phone, laptop) = phone_and_laptop(&lookup).await;
    eventually("token for the laptop", || {
        issued_for_own(&phone, &laptop).is_some()
    })
    .await;
    let kicks = Arc::new(Kicks::default());
    let hooks: Arc<dyn NodeHooks> = kicks.clone();
    phone
        .ctx
        .install_hooks(Arc::downgrade(&hooks) as Weak<dyn NodeHooks>);
    // The push the relay would send the phone for the laptop's wake: the tag issued to it.
    let (key, tag) = (
        phone.state(|st| st.wake.as_ref().unwrap().hint_key),
        issued_for_own(&phone, &laptop).unwrap().tag,
    );
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&proto::new_device_secret()[..12]);
    let hint = wake_relay_core::hint::seal(
        &key,
        &wake_relay_core::Hint {
            tag,
            kind: Kind::Msg,
            sent_at_ms: now_ms(),
        },
        nonce,
    )
    .unwrap();
    let action = phone.ctx.on_wake(&hint).unwrap();
    assert_eq!(
        action,
        WakeAction::DialBack { did: phone.did() },
        "the app sees a dial-back under our own DID"
    );
    assert!(kicks.0.lock().is_empty(), "no contact is dialled");
    assert_eq!(
        kicks.2.load(Ordering::SeqCst),
        1,
        "pending pictures are retried at once"
    );
    assert!(!phone.ctx.wake_busy(), "nothing queued for a contact");
}
