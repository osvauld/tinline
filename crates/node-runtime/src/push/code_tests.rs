//! Code wakes and the queued contact request on in-process nodes (`docs/push.md` "Code wake
//! tokens", "Tokens at setup"): real iroh endpoints on loopback, the fake relay of `tests.rs`. A
//! "sleeping phone" is a node whose endpoint is closed; the fake relay's wake hook opens the hint
//! on it (`on_wake`) and starts it again. Names start with `wake_` (`cargo test -p node-runtime
//! --lib wake`).

use std::time::Duration;

use iroh::address_lookup::memory::MemoryLookup;
use proto::ContactTicket;
use wake::WakeAction;
use wake_relay_core::Hint;

use super::tests::*;
use super::*;
use crate::caps::ContactOrigin;
use crate::contact_req::{AddOutcome, ContactRequest};
use crate::store::{Disk, ProfileV2};
use crate::{Contact, ContactRequestSink};

/// Setup dial timeout in these tests: a closed loopback endpoint never answers.
const DIAL_MS: u64 = 1_500;

/// Lets `persist_*` succeed without account files (an identity not yet committed writes
/// nothing), as the add and accept paths need.
fn persistable(t: &T) {
    let mut s = t.ctx.shared.lock();
    s.disk = Some(Disk::V2(ProfileV2 {
        version: 2,
        name: "synthetic".into(),
        did: t.did(),
        device_public: t.me.device,
        vault: account_model::vault::Vault {
            salt: vec![],
            m: 0,
            t: 0,
            p: 0,
            wrapped_dek: vec![],
            sealed: vec![],
        },
        device_id: None,
        profile_name_pending: false,
        profile_inherited: false,
    }));
    s.committed = false;
    drop(s);
    t.ctx.set_setup_dial_timeout(DIAL_MS);
}

pub(super) async fn setup_node(lookup: &MemoryLookup) -> T {
    let t = node(lookup).await;
    persistable(&t);
    t
}

/// What the app would have been told about contact requests.
#[derive(Default)]
struct Reqs {
    requests: Mutex<Vec<ContactRequest>>,
    accepted: Mutex<Vec<Contact>>,
    /// Run when a request is accepted (what the node holds at that moment).
    probe: Mutex<Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>>,
    probed: Mutex<Vec<Option<String>>>,
}

impl ContactRequestSink for Reqs {
    fn on_contact_request(&self, request: ContactRequest) {
        self.requests.lock().push(request);
    }
    fn on_request_accepted(&self, contact: Contact) {
        let probe = self.probe.lock().clone();
        if let Some(p) = probe {
            self.probed.lock().push(p());
        }
        self.accepted.lock().push(contact);
    }
}

fn listen(t: &T) -> Arc<Reqs> {
    let r = Arc::new(Reqs::default());
    *t.ctx.requests.events.lock() = Some(r.clone());
    r
}

/// The push the relay would send `owner` for a wake with this (fake relay) token.
fn hint_for(owner: &T, token: &str, kind: Kind) -> String {
    let key = owner.state(|st| st.wake.as_ref().unwrap().hint_key);
    let tag = fake_tag(token).expect("a token of the fake relay");
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&proto::new_device_secret()[..12]);
    wake_relay_core::hint::seal(
        &key,
        &Hint {
            tag,
            kind,
            sent_at_ms: now_ms(),
        },
        nonce,
    )
    .unwrap()
}

/// Every wake `sender` asks its relay for reaches `phone` as a push: `phone` opens it (`on_wake`,
/// outcomes recorded) and, unless it ignored it, starts after `delay`.
fn phone_wakes(
    sender: &T,
    phone: &T,
    lookup: &MemoryLookup,
    delay: Duration,
) -> Arc<Mutex<Vec<WakeAction>>> {
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let (ctx, secret, lookup, seen) = (
        phone.ctx.clone(),
        phone.secret,
        lookup.clone(),
        outcomes.clone(),
    );
    let key = phone.state(|st| st.wake.as_ref().map(|w| w.hint_key));
    *sender.relay.on_wake.lock() = Some(Arc::new(move |token, kind| {
        let Some(key) = key else { return };
        let Some(tag) = fake_tag(token) else { return };
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&proto::new_device_secret()[..12]);
        let hint = wake_relay_core::hint::seal(
            &key,
            &Hint {
                tag,
                kind,
                sent_at_ms: now_ms(),
            },
            nonce,
        )
        .unwrap();
        let action = ctx.on_wake(&hint).unwrap();
        let start = !matches!(action, WakeAction::Ignore | WakeAction::IgnoreCall);
        seen.lock().push(action);
        if start {
            let (ctx, lookup) = (ctx.clone(), lookup.clone());
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                bring_up(&ctx, secret, &lookup).await;
            });
        }
    }));
    outcomes
}

pub(super) fn claim_of(code: &str) -> proto::InviteClaim {
    ContactTicket::from_text(code)
        .unwrap()
        .verify(crate::now())
        .unwrap()
}

/// A realistic code: a 163-character FCM token sealed by the real relay logic, in a ticket with
/// a known relay. Prints the sizes (`--nocapture`) for `docs/push.md`.
#[test]
fn wake_code_ticket_size_with_a_real_fcm_sized_token() {
    use wake_relay_core::api::{RegisterRequest, TagRequest};
    let fcm = format!("{}:APA91b{}", "c".repeat(22), "x".repeat(134));
    assert_eq!(fcm.len(), 163);
    let key = wake_relay_core::RelayKey {
        kid: 1,
        key: [3; 32],
    };
    let now = now_ms();
    let req = RegisterRequest {
        target: Target::Fcm { token: fcm },
        hint_key: [5; 32],
        tags: vec![TagRequest {
            tag: [6; 16],
            kinds: Kinds::MSG,
            exp_ms: now + 7 * 86_400_000,
        }],
    };
    let resp = wake_relay_core::relay::register(&key, &req, now, || [7; 12]).unwrap();
    let raw = wake::token_bytes(&resp.tokens[0].token).expect("canonical base64url");
    assert!(raw.len() <= proto::MAX_TICKET_WAKE, "{}", raw.len());
    let (id, _) = identity::generate();
    let relay = Some("https://euc1-1.relay.n0.iroh.link./".to_string());
    let (nonce, exp_min) = proto::ticket_nonce_and_exp(now / 1000, crate::TICKET_TTL);
    let name = "Alexandria Montgome";
    let plain =
        proto::issue_contact_ticket_with(&id, [1; 32], name, relay.clone(), nonce, exp_min, None)
            .to_text();
    let with = proto::issue_contact_ticket_with(
        &id,
        [1; 32],
        name,
        relay,
        nonce,
        exp_min,
        Some(raw.clone()),
    )
    .to_text();
    eprintln!(
        "code wake token: {} chars base64url, {} raw bytes; code: {} chars without, {} with",
        resp.tokens[0].token.len(),
        raw.len(),
        plain.len(),
        with.len()
    );
    assert!(
        plain.len() <= 265 && with.len() <= 700,
        "{} {}",
        plain.len(),
        with.len()
    );
    assert_eq!(claim_of(&with).wake, Some(raw));
}

// (i) A asleep in push mode, B adds by code: B wakes A with the code's token, A's node starts,
// the queued request lands, A's user is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_code_i_a_sleeping_owner_is_woken_and_the_request_lands() {
    let lookup = MemoryLookup::new();
    let (a, b) = (setup_node(&lookup).await, setup_node(&lookup).await);
    set_target(&a, Some(fcm(1)));
    let reqs = listen(&a);
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    let claim = claim_of(&code);
    let token = a.state(|st| {
        st.wake
            .as_ref()
            .unwrap()
            .code_for(&claim.nonce)
            .unwrap()
            .token
            .clone()
    });
    assert_eq!(
        claim.wake.as_deref().map(wake::token_text),
        Some(token.clone()),
        "the code carries its token"
    );
    let reg = a.relay.registers.lock().last().cloned().unwrap();
    assert_eq!(
        (reg.tags.len(), reg.tags[0].kinds),
        (1, Kinds::MSG),
        "message wakes only"
    );
    assert_eq!(
        reg.tags[0].exp_ms,
        claim.exp * 1000,
        "expires with the code"
    );
    // The same code is handed out again while it matches.
    assert_eq!(a.ctx.ticket_with_relay(None).await.unwrap(), code);

    a.ep.close().await;
    let woke = phone_wakes(&b, &a, &lookup, Duration::from_millis(500));
    let out = b
        .ctx
        .clone()
        .add_contact_outcome(code, ContactOrigin::Ticket)
        .await
        .unwrap();
    assert!(
        matches!(&out, AddOutcome::Requested { did, .. } if *did == a.did()),
        "queued, shown as requested"
    );

    eventually("A's user is asked", || reqs.requests.lock().len() == 1).await;
    assert_eq!(reqs.requests.lock()[0].did, b.did());
    assert_eq!(
        b.relay.wakes.lock()[0],
        (token, Kind::Msg),
        "B woke A with the code's token"
    );
    assert_eq!(woke.lock()[0], WakeAction::CodeWake);
    eventually("B's request is delivered", || {
        b.state(|st| {
            st.sent_requests
                .get(&a.did())
                .is_some_and(|r| r.delivered && r.ticket.is_none())
        })
    })
    .await;
    assert!(
        b.relay.wakes.lock().len() <= 2,
        "one wake per 30 s at most: {:?}",
        b.relay.wakes.lock().len()
    );
}

// (ii) A code without a token (no push): A down, then up; the queued request is delivered later,
// with no wake.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_code_ii_a_request_without_a_token_is_queued_and_delivered_later() {
    let lookup = MemoryLookup::new();
    let (a, b) = (setup_node(&lookup).await, setup_node(&lookup).await);
    let reqs = listen(&a);
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    assert_eq!(claim_of(&code).wake, None, "no push, no token");
    a.ep.close().await;

    let out = b
        .ctx
        .clone()
        .add_contact_outcome(code, ContactOrigin::Ticket)
        .await
        .unwrap();
    assert!(matches!(out, AddOutcome::Requested { .. }));
    assert!(
        b.state(|st| st
            .sent_requests
            .get(&a.did())
            .is_some_and(|r| !r.delivered && r.ticket.is_some())),
        "queued"
    );
    // The queue is in the sealed state.
    let saved: State =
        serde_json::from_str(&serde_json::to_string(&b.state(|st| st.clone())).unwrap()).unwrap();
    assert!(saved.sent_requests[&a.did()].ticket.is_some());

    tokio::time::sleep(Duration::from_secs(2)).await;
    bring_up(&a.ctx, a.secret, &lookup).await;
    eventually("A's user is asked", || reqs.requests.lock().len() == 1).await;
    eventually("delivered", || {
        b.state(|st| st.sent_requests.get(&a.did()).is_some_and(|r| r.delivered))
    })
    .await;
    assert!(b.relay.wakes.lock().is_empty(), "nothing to wake with");
}

/// B adds A by code (both awake), A holds the request. Both have push.
async fn requested_pair(lookup: &MemoryLookup) -> (T, T, Arc<Reqs>, Arc<Reqs>) {
    let (a, b) = (setup_node(lookup).await, setup_node(lookup).await);
    set_target(&a, Some(fcm(1)));
    set_target(&b, Some(fcm(2)));
    let (ra, rb) = (listen(&a), listen(&b));
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    let out = b
        .ctx
        .clone()
        .add_contact_outcome(code, ContactOrigin::Ticket)
        .await
        .unwrap();
    assert!(matches!(out, AddOutcome::Requested { .. }));
    eventually("A's user is asked", || ra.requests.lock().len() == 1).await;
    (a, b, ra, rb)
}

// (iii) A accepts while B is asleep: the Welcome wakes B with the token from B's hello.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_code_iii_the_welcome_wakes_a_sleeping_joiner() {
    let lookup = MemoryLookup::new();
    let (a, b, _ra, rb) = requested_pair(&lookup).await;
    let b_token = b
        .issued_to(&a)
        .expect("B issued a token for its open request")
        .token;
    let hello_token = a.state(|st| {
        crate::push::handover_in(&st.contact_requests[&b.did()].hello).map(|h| h.token.clone())
    });
    assert_eq!(
        hello_token.as_deref(),
        Some(b_token.as_str()),
        "B's hello carried it"
    );

    b.ep.close().await;
    let woke = phone_wakes(&a, &b, &lookup, Duration::from_millis(500));
    a.ctx.accept_request(&a.me, &b.did()).unwrap();
    assert_eq!(
        a.held_from(&b).map(|h| h.token),
        Some(b_token.clone()),
        "kept with the contact at Accept"
    );
    a.ctx.welcome_kick(b.did());

    eventually("B hears it was accepted", || rb.accepted.lock().len() == 1).await;
    assert!(
        a.relay.wakes.lock().contains(&(b_token, Kind::Msg)),
        "A woke B with B's token"
    );
    assert_eq!(
        woke.lock()[0],
        WakeAction::DialBack { did: a.did() },
        "B knows who woke it"
    );
    assert!(b.state(|st| st.contacts.iter().any(|c| c.did == a.did())));
    eventually("the welcome outbox empties", || {
        a.state(|st| st.welcome_outbox.is_empty())
    })
    .await;
}

// (iv) After the setup both hold each other's token, without the `WakeTokens` round.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_code_iv_tokens_both_ways_at_setup() {
    let lookup = MemoryLookup::new();
    let (a, b, _ra, rb) = requested_pair(&lookup).await;
    // A holds B's token the moment it accepts: B cannot have sent `WakeTokens` (A was no contact).
    a.ctx.accept_request(&a.me, &b.did()).unwrap();
    assert_eq!(
        a.held_from(&b).map(|h| h.token),
        b.issued_to(&a).map(|i| i.token)
    );
    // B holds A's token the moment the Welcome is taken.
    let (bctx, adid, adev) = (b.ctx.clone(), a.did(), a.me.device);
    *rb.probe.lock() = Some(Arc::new(move || {
        bctx.shared
            .lock()
            .state
            .wake
            .as_ref()?
            .held
            .iter()
            .find(|e| e.did == adid && e.device == adev)
            .map(|e| e.held.token.clone())
    }));
    a.ctx.welcome_kick(b.did());
    eventually("B hears it was accepted", || rb.accepted.lock().len() == 1).await;
    let a_token = a.issued_to(&b).expect("A issued B a token").token;
    assert_eq!(
        rb.probed.lock().clone(),
        vec![Some(a_token)],
        "the Welcome carried it"
    );
}

// (v) A reset code takes its token back at the relay; an expired one is just forgotten.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_code_v_a_reset_code_revokes_its_token() {
    let lookup = MemoryLookup::new();
    let a = setup_node(&lookup).await;
    set_target(&a, Some(fcm(1)));
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    let nonce = claim_of(&code).nonce;
    let token = a.state(|st| {
        st.wake
            .as_ref()
            .unwrap()
            .code_for(&nonce)
            .unwrap()
            .token
            .clone()
    });
    // A wake round leaves a live code alone.
    a.ctx.wake_kick();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(a.relay.revokes.lock().is_empty());

    a.edit(|st| crate::retire_ticket(st, false));
    a.ctx.wake_kick();
    eventually("the code token is revoked", || {
        a.relay.revokes.lock().contains(&token)
    })
    .await;
    assert!(a.state(|st| st.wake.as_ref().unwrap().codes.is_empty()));
    // The next code carries a fresh token.
    let next = a.ctx.ticket_with_relay(None).await.unwrap();
    let w = claim_of(&next).wake.map(|w| wake::token_text(&w));
    assert!(w.is_some() && w.as_deref() != Some(token.as_str()));

    // Push turned off: the code is replaced by one without a token, the old token revoked.
    let live = w.unwrap();
    set_target(&a, None);
    let plain = a.ctx.ticket_with_relay(None).await.unwrap();
    assert_eq!(claim_of(&plain).wake, None);
    eventually("revoked with push off", || {
        a.relay.revokes.lock().contains(&live)
    })
    .await;
}

// A code wake on a node: the outcome, a hint for a call or past the throttle is ignored.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_code_on_wake_outcomes() {
    let lookup = MemoryLookup::new();
    let a = setup_node(&lookup).await;
    set_target(&a, Some(fcm(1)));
    let code = a.ctx.ticket_with_relay(None).await.unwrap();
    let token = wake::token_text(&claim_of(&code).wake.unwrap());
    assert_eq!(
        a.ctx.on_wake(&hint_for(&a, &token, Kind::Call)).unwrap(),
        WakeAction::IgnoreCall
    );
    for _ in 0..wake::CODE_WAKE_CAP {
        assert_eq!(
            a.ctx.on_wake(&hint_for(&a, &token, Kind::Msg)).unwrap(),
            WakeAction::CodeWake
        );
    }
    assert_eq!(
        a.ctx.on_wake(&hint_for(&a, &token, Kind::Msg)).unwrap(),
        WakeAction::Ignore,
        "throttled"
    );
}
