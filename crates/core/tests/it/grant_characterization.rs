//! W2-C: characterisation of what a contact needs TODAY to message and to call, measured against
//! the user's decision U13 (`docs/design/rotation-open-decisions.md` §0): separate `message` and
//! `call` capability tokens; ticket contacts get both, group chat-request contacts message only;
//! blocking revokes both and sends a signed visible notice (U5 revised).
//!
//! Today (verified in source):
//! - One grant, `proto::Grant { cap: "call" }` (`crates/proto/src/lib.rs:34`, `:542`), issued
//!   by each side to the other inside `ContactHello` / `ContactWelcome` (`lib.rs:850`, `:899`)
//!   with `GRANT_TTL` = 1 year (`crates/core/src/node.rs:33`) and stored as
//!   `StoredContact.grant_from_them`.
//! - Calls: `incoming_call` runs `proto::accept_call_hello` (`node.rs:2162`): attested device =
//!   connection key, grant verifies (issuer us, holder the attested DID, cap "call", unexpired,
//!   id not in `State.revoked`), and the DID is a contact that is not blocked. Any failure is
//!   answered `Reject { reason: "not accepted" }` (`node.rs:67`, `:2173`).
//! - Chat: `verify_auth` (`crates/core/src/chat/engine.rs:331`) re-wraps the chat `Auth` as a
//!   `CallHello` and runs the SAME `accept_call_hello`, with the same grant. Both sides present
//!   the grant the other issued (`engine.rs:390`, `:410`). Failure closes the connection
//!   ("not accepted", `engine.rs:466`).
//! - Renewal: only the callee's `Accept` carries `renewed_grant` (`node.rs:2753`); the caller keeps
//!   it if it outlives the stored one (`node.rs:2839`, `grant_outlives` `:3153`).
//! - Revocation: `State.revoked` (`crates/core/src/store.rs:169`) is read on both paths and
//!   merged between own installs (`crates/account-model/src/sync/accdoc.rs`), but no code path
//!   adds to it. Blocking is per DID (`State.blocked`, `Node::remove_contact` `node.rs:984`).
//!
//! P6 (U13, U15) built separate tokens: the `KIND: limitation` tests below were flipped to the
//! decided behaviour (each says what it asserted before). Today: ticket contacts exchange a
//! `message` and a `call` token (version 2, one cap each); chat checks the message token, calls
//! the call token; a contact without call permission is answered "calls not allowed"; tokens are
//! taken back per DID with cut-offs (block: both; "disallow calls": call), never by id (the
//! unused id set `State.revoked` was deleted, 2026-10-15).
//!
//! Every test carries one label:
//! - `KIND: limitation`: asserts today's outcome where it differs from U13; the comment names the
//!   expected-correct outcome. When production is fixed the assertion flips and the test must be
//!   rewritten.
//! - `KIND: invariant`: holds today and must keep holding under U13.
//! - `KIND: candidate-policy G-RENEWAL`: today's renewal rule, one option of an open gate.
//!
//! Real iroh endpoints (needs network, like `calls.rs`, `block_characterization.rs`). Some tests
//! drive a node with a hand-rolled "raw" peer that speaks the wire protocol through `proto` so
//! the grant bytes can be inspected and swapped; it never writes into a node's state. Waits are
//! bounded. No production code is changed; the shared harness is in `support/`.

use std::time::{Duration, Instant};

use identity::Identity;
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey};
use p2pcore::{CallInfo, CallState, DeliveryState};
use proto::Msg;
use serde::{Deserialize, Serialize};

use crate::support;
use support::eventually_200ms as eventually;
use support::wait::now_ms as now;
use support::*;

/// `GRANT_TTL` (`crates/core/src/node.rs:33`), private there.
const GRANT_TTL: u64 = 365 * 24 * 3600 * 1000;
/// `CHAT_ALPN` (`crates/core/src/chat/wire.rs`), private there. Version 7 since postcard.
const CHAT_ALPN: &[u8] = b"tinline/chat/7";
const WAIT: Duration = Duration::from_secs(30);
const S: Suite = Suite("w2c");

impl Peer {
    fn grant_dump(&self) -> String {
        format!("{:?} {:?}", self.node.contacts(), self.node.blocked_for_test())
    }
}

fn ended_within(p: &Peer, id: &str, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::State(i, CallState::Ended { reason })) if i == id => return reason,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for call {id} to end"),
        }
    }
}

fn incoming_within(p: &Peer, secs: u64) -> CallInfo {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match p
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(Ev::Incoming(c)) => return c,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for an incoming call"),
        }
    }
}

fn delivery(p: &Peer, did: &str, id: &str) -> DeliveryState {
    all_msgs(p, did)
        .into_iter()
        .find(|m| m.id == id)
        .expect("message held")
        .delivery
}

/// The chat `Auth` frame, mirrored: variant 0 of `ChatMsg` (`crates/core/src/chat/wire.rs:30`),
/// postcard (`cryptography::codec`), u32 BE length prefix. Only the first frame of a session is read, which is `Auth`.
#[derive(Debug, Serialize, Deserialize)]
enum ChatAuth {
    Auth {
        attestation: proto::SignedAttestation,
        grant: proto::SignedGrant,
        relay: Option<String>,
    },
}

/// A peer that speaks the call/contact protocol directly through `proto`, so tests can choose
/// which grant bytes it presents. Its endpoint accepts nothing (no ALPNs).
struct Raw {
    rt: tokio::runtime::Runtime,
    id: Identity,
    device: [u8; 32],
    ep: Endpoint,
}

/// A raw call in progress: the connection and its control stream.
struct RawCall {
    conn: Connection,
    send: SendStream,
    recv: RecvStream,
    buf: Vec<u8>,
}

/// What a raw contact holds after adding a node.
struct Added {
    /// The node's message token to the raw peer (what it presents to chat with the node).
    from_node: proto::SignedGrant,
    /// The node's call token to the raw peer (what it presents to call the node).
    call_from_node: Option<proto::SignedGrant>,
    /// The raw peer's message token to the node.
    to_node: proto::SignedGrant,
    addr: EndpointAddr,
}

async fn recv_msg(recv: &mut RecvStream, buf: &mut Vec<u8>) -> Option<Msg> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some((m, used)) = proto::decode_frame(buf).ok()? {
            buf.drain(..used);
            return Some(m);
        }
        match recv.read(&mut chunk).await {
            Ok(Some(n)) => buf.extend_from_slice(&chunk[..n]),
            _ => return None,
        }
    }
}

impl Raw {
    fn new() -> Raw {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (id, _) = identity::generate();
        let secret = proto::new_device_secret();
        let device = cryptography::signature::public_key(&secret);
        let ep = rt.block_on(async {
            Endpoint::builder(presets::N0)
                .secret_key(SecretKey::from_bytes(&secret))
                .bind()
                .await
                .unwrap()
        });
        Raw { rt, id, device, ep }
    }

    fn did(&self) -> String {
        self.id.did().to_string()
    }

    fn attestation(&self) -> proto::SignedAttestation {
        proto::attest(&self.id, self.device, now() / 1000)
    }

    /// Redeems `ticket` like `Node::add_contact` does (`node.rs:1952`), through `proto`.
    fn add(&self, ticket: &str) -> Added {
        let ticket = proto::ContactTicket::from_text(ticket).unwrap();
        let (hello, pending) =
            proto::contact_hello(&self.id, self.device, &ticket, "raw", now(), GRANT_TTL).unwrap();
        let Msg::ContactHello { grant_for_you, .. } = &hello else {
            unreachable!()
        };
        let to_node = grant_for_you.clone();
        let device = ticket.device;
        let mut addr = EndpointAddr::new(PublicKey::from_bytes(&device).unwrap());
        if let Some(url) = ticket
            .relay
            .as_deref()
            .and_then(|r| r.parse::<RelayUrl>().ok())
        {
            addr = addr.with_relay_url(url);
        }
        let (from_node, call_from_node) = self.rt.block_on(async {
            let conn = tokio::time::timeout(WAIT, self.ep.connect(addr.clone(), proto::ALPN))
                .await
                .expect("dial in time")
                .expect("dial");
            let remote = *conn.remote_id().as_bytes();
            let (mut send, mut recv) = conn.open_bi().await.unwrap();
            send.write_all(&proto::encode_frame(&hello)).await.unwrap();
            let mut buf = Vec::new();
            let reply = tokio::time::timeout(WAIT, recv_msg(&mut recv, &mut buf))
                .await
                .expect("welcome in time")
                .expect("welcome");
            let new = proto::accept_contact_welcome(&self.id, &pending, &reply, remote, now())
                .expect("welcome verifies");
            conn.close(0u32.into(), b"added");
            (new.grant_from_them, new.call_grant_from_them)
        });
        Added {
            from_node,
            call_from_node,
            to_node,
            addr,
        }
    }

    /// Sends a `CallHello` presenting `grant`; returns the call and the node's first reply
    /// (`None` if the stream ended without one).
    fn call(
        &self,
        addr: &EndpointAddr,
        grant: &proto::SignedGrant,
        call_id: &str,
    ) -> (RawCall, Option<Msg>) {
        self.rt.block_on(async {
            let conn = tokio::time::timeout(WAIT, self.ep.connect(addr.clone(), proto::ALPN))
                .await
                .expect("dial in time")
                .expect("dial");
            let (mut send, mut recv) = conn.open_bi().await.unwrap();
            let hello =
                proto::call_hello(self.attestation(), grant.clone(), call_id.into());
            send.write_all(&proto::encode_frame(&hello)).await.unwrap();
            let mut buf = Vec::new();
            let first = tokio::time::timeout(WAIT, recv_msg(&mut recv, &mut buf))
                .await
                .expect("reply in time");
            (
                RawCall {
                    conn,
                    send,
                    recv,
                    buf,
                },
                first,
            )
        })
    }

    fn next(&self, c: &mut RawCall) -> Option<Msg> {
        self.rt.block_on(async {
            tokio::time::timeout(WAIT, recv_msg(&mut c.recv, &mut c.buf))
                .await
                .expect("next message in time")
        })
    }

    fn hang_up(&self, mut c: RawCall) {
        self.rt.block_on(async {
            let _ = c.send.write_all(&proto::encode_frame(&Msg::Hangup)).await;
            let _ = c.send.finish();
            c.conn.close(0u32.into(), b"bye");
        });
    }

    /// Opens a chat session presenting `grant`; the node's own `Auth` if it accepted, `None` if
    /// it closed the connection instead.
    fn chat_auth(
        &self,
        addr: &EndpointAddr,
        grant: &proto::SignedGrant,
    ) -> Option<(proto::SignedAttestation, proto::SignedGrant)> {
        self.rt.block_on(async {
            let conn = tokio::time::timeout(WAIT, self.ep.connect(addr.clone(), CHAT_ALPN))
                .await
                .expect("dial in time")
                .expect("dial");
            let (mut send, mut recv) = conn.open_bi().await.unwrap();
            let auth = ChatAuth::Auth {
                attestation: self.attestation(),
                grant: grant.clone(),
                relay: None,
            };
            let body = cryptography::codec::to_bytes(&auth).unwrap();
            let mut frame = (body.len() as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&body);
            send.write_all(&frame).await.unwrap();
            let read = async {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    if buf.len() >= 4 {
                        let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
                        if buf.len() >= 4 + len {
                            let ChatAuth::Auth {
                                attestation, grant, ..
                            } = cryptography::codec::from_bytes(&buf[4..4 + len], 1 << 24).ok()?;
                            return Some((attestation, grant));
                        }
                    }
                    match recv.read(&mut chunk).await {
                        Ok(Some(n)) => buf.extend_from_slice(&chunk[..n]),
                        _ => return None,
                    }
                }
            };
            let out = tokio::time::timeout(WAIT, read)
                .await
                .expect("auth reply in time");
            conn.close(0u32.into(), b"done");
            out
        })
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        self.rt.block_on(self.ep.close());
    }
}

fn verify_from(node: &Peer, holder: &str, g: &proto::SignedGrant) -> proto::Grant {
    g.redeem(&proto::Expect { iss: Some(&node.did), holder, now_ms: now() }, &proto::Unrecorded).expect("grant verifies")
}

fn is_reject(m: &Option<Msg>) -> bool {
    matches!(m, Some(Msg::Reject { reason }) if reason == "not accepted")
}

// ---- tests ----

// KIND: invariant (flipped by P6; was `ticket_contact_holds_one_call_grant_with_a_one_year_ttl`,
// which asserted ONE grant with cap "call" and no messaging capability)
// U13: a ticket contact receives two separate DID-signed tokens from the node, `message` and
// `call`, each with a one-year TTL, issued by the node's DID to the contact's.
#[test]
fn ticket_contact_holds_message_and_call_tokens_with_a_one_year_ttl() {
    let a = S.peer("ttl-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    let m = verify_from(&a, &raw.did(), &added.from_node);
    let c = verify_from(&a, &raw.did(), added.call_from_node.as_ref().expect("a call token"));
    assert_eq!((m.claim, c.claim), (proto::Cap::Message, proto::Cap::Call));
    assert_ne!(m.nonce, c.nonce);
    for g in [&m, &c] {
        assert_eq!(g.exp_ms - g.iat_ms, GRANT_TTL);
        assert_eq!((g.iss.as_str(), g.holder()), (a.did.as_str(), Some(raw.did().as_str())));
    }
    assert!(a.node.contacts().iter().any(|c| c.did == raw.did()));
    assert!(a.node.calls_allowed(raw.did()));
}

// KIND: invariant (flipped by P6; was `one_call_grant_gates_both_calls_and_chat`, which
// asserted that the one "call" grant both rang the node and opened chat)
// U13/U15: the call token rings but does not open chat; the message token opens chat (the node
// answers with its own `Auth`, presenting the message token WE issued it) but a call presenting
// it is answered "calls not allowed" and never rings. A self-issued token opens neither.
#[test]
fn the_message_token_opens_chat_and_the_call_token_rings() {
    let a = S.peer("both-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    let call_token = added.call_from_node.clone().unwrap();

    // Call path: the call token rings, the message token is told "calls not allowed".
    let (call, first) = raw.call(&added.addr, &call_token, "grantcall1");
    assert!(matches!(first, Some(Msg::Ringing)), "rings: {first:?}");
    assert_eq!(incoming_within(&a, 10).peer_did, raw.did());
    raw.hang_up(call);
    let (_, first) = raw.call(&added.addr, &added.from_node, "grantcall3");
    assert!(
        matches!(&first, Some(Msg::Reject { reason }) if reason == p2pcore::CALLS_NOT_ALLOWED),
        "{first:?}"
    );

    // Chat path: the message token is accepted, the call token is not.
    let (att, grant_back) = raw
        .chat_auth(&added.addr, &added.from_node)
        .expect("chat accepted");
    let att = proto::verify_attestation(&att).unwrap();
    assert_eq!(att.did, a.did);
    assert_eq!(
        grant_back, added.to_node,
        "the node presents our message token, it issues nothing"
    );
    assert!(raw.chat_auth(&added.addr, &call_token).is_none());

    // A token raw issued itself (wrong issuer) opens neither.
    let own = proto::issue_grant(&raw.id, &raw.did(), now(), GRANT_TTL);
    let (_, first) = raw.call(&added.addr, &own, "grantcall2");
    assert!(is_reject(&first), "{first:?}");
    assert!(raw.chat_auth(&added.addr, &own).is_none());
    assert!(a.logged("rejected call: issuer is not who was expected"));
}

// KIND: invariant
// A grant is bound to its holder: contact C presenting contact B's grant (lifted bytes) is
// refused on both paths with the generic reason, nothing rings, and the node's contacts and
// block list are unchanged.
#[test]
fn a_lifted_grant_is_refused_on_both_paths_with_state_unchanged() {
    let a = S.peer("lift-a");
    let b = Raw::new();
    let c = Raw::new();
    let b_added = b.add(&a.node.my_ticket().unwrap());
    let c_added = c.add(&a.node.my_ticket().unwrap());
    let before = a.grant_dump();
    let (_, first) = c.call(&c_added.addr, b_added.call_from_node.as_ref().unwrap(), "liftcall1");
    assert!(is_reject(&first), "{first:?}");
    assert!(c.chat_auth(&c_added.addr, &b_added.from_node).is_none());
    assert!(a.logged("rejected call: holder is not who was expected"));
    assert!(!saw_incoming(&a));
    assert_eq!(a.grant_dump(), before);
}

// KIND: invariant (P6 kept the renewal rule for the call token, section 14 Q3)
// When the callee answers, its `Accept` carries a fresh call token for the caller (new id, `iat`
// = now, one-year TTL). A declined call carries none. (Message tokens renew on chat activity near
// expiry, PROVISIONAL: `caps.rs` `RENEW_WINDOW`.)
#[test]
fn answering_renews_the_callers_grant_declining_does_not() {
    let a = S.peer("renew-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    let call_token = added.call_from_node.clone().unwrap();
    let old = verify_from(&a, &raw.did(), &call_token);

    let (mut call, first) = raw.call(&added.addr, &call_token, "renewcall1");
    assert!(matches!(first, Some(Msg::Ringing)), "{first:?}");
    let info = incoming_within(&a, 10);
    let t_answer = now();
    a.node.answer(info.call_id).unwrap();
    let accept = raw.next(&mut call);
    let Some(Msg::Accept {
        renewed_grant: Some(g),
        ..
    }) = accept
    else {
        panic!("expected Accept with a renewed grant, got {accept:?}")
    };
    let new = verify_from(&a, &raw.did(), &g);
    assert_eq!(new.claim, proto::Cap::Call);
    assert_ne!(new.nonce, old.nonce, "a renewal is a new grant");
    assert!(new.iat_ms >= t_answer && new.iat_ms >= old.iat_ms);
    assert_eq!(new.exp_ms - new.iat_ms, GRANT_TTL);
    raw.hang_up(call);

    let (mut call, first) = raw.call(&added.addr, &call_token, "renewcall2");
    assert!(matches!(first, Some(Msg::Ringing)), "{first:?}");
    let info = incoming_within(&a, 10);
    a.node.decline(info.call_id).unwrap();
    let reply = raw.next(&mut call);
    assert!(matches!(reply, Some(Msg::Decline { .. })), "{reply:?}");
    raw.hang_up(call);
}

// KIND: invariant (flipped by P5+P6; was `block_refuses_by_did_with_the_generic_reason`, which
// asserted the generic "not accepted" and a grant left untouched)
// U13 + U5 revised: while blocked, both paths answer with the one signed notice; the block took
// both tokens back (per-DID cut-off), so after the unblock the same tokens are refused with the
// generic reason on both paths, nothing rings, and the node's state is unchanged by the attempts.
#[test]
fn block_takes_both_tokens_back_and_unblock_does_not_revive_them() {
    let a = S.peer("blk-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    let call_token = added.call_from_node.clone().unwrap();
    a.node.block(raw.did()).unwrap();
    let (_, first) = raw.call(&added.addr, &call_token, "blkcall0");
    assert!(matches!(first, Some(Msg::Notice { .. })), "{first:?}");
    a.node.unblock(raw.did()).unwrap();
    let before = a.grant_dump();
    // The tokens still verify cryptographically: the refusal is the issuer's cut-off.
    verify_from(&a, &raw.did(), &added.from_node);
    for id in ["blkcall1", "blkcall2"] {
        let (_, first) = raw.call(&added.addr, &call_token, id);
        assert!(is_reject(&first), "{first:?}");
    }
    assert!(raw.chat_auth(&added.addr, &added.from_node).is_none());
    assert!(a.logged("incoming call refused: not accepted"));
    assert!(!saw_incoming(&a));
    assert_eq!(a.grant_dump(), before);
}

// KIND: invariant (P6 decided section 14 Q2: per-DID cut-off, not ids; was
// `no_code_path_writes_the_grant_revocation_set`, a limitation)
// Tokens are taken back per DID (`State::cut_offs`); there is no id set to revoke by (every
// renewal mints a new token, so ids would leak). Source scan of every non-test file under
// `crates/core/src` and the runtime crates it is split into (`node-runtime`, `chat-runtime`,
// `group-runtime`), walked at run time so split modules stay covered.
#[test]
fn tokens_are_taken_back_per_did_never_by_id() {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                if name != "tests" {
                    walk(&p, out);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
                out.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&core.join("src"), &mut files);
    for rt in ["node-runtime", "chat-runtime", "group-runtime"] {
        walk(&core.join("..").join(rt).join("src"), &mut files);
    }
    assert!(files.len() > 50, "source scan found only {} files", files.len());
    let ids: Vec<_> = files
        .iter()
        .flat_map(|(f, t)| t.lines().filter(|l| !l.trim().starts_with("//") && l.contains(".revoked")).map(move |l| (f, l)))
        .collect();
    assert!(ids.is_empty(), "something keeps a revoked-id set: {ids:?}");
    // The cut-offs are what a block and "disallow calls" raise.
    let (caps, rt_caps) = (include_str!("../../src/caps.rs"), include_str!("../../../node-runtime/src/caps.rs"));
    assert!(caps.contains("cut.call = cut.call.max(now_ms())") && rt_caps.contains("cut.message = cut.message.max(t)"));
}

// KIND: invariant (flipped by P6; was `only_global_availability_separates_calls_from_messages`,
// which asserted that only the global "not now" switch could refuse calls but keep messages)
// U13: per contact. With calls disallowed for b, b's call is refused with the visible reason
// "calls not allowed" and never rings while its messages are delivered; another contact c still
// rings a. The global availability switch still works as before.
#[test]
fn calls_are_refused_per_contact_while_messages_flow() {
    let (a, b) = S.pair("away");
    let c = S.peer("away-c");
    c.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    a.node.allow_calls(b.did.clone(), false).unwrap();
    let call = b.node.call(a.did.clone()).unwrap();
    assert_eq!(ended_within(&b, &call.call_id, 40), p2pcore::CALLS_NOT_ALLOWED);
    assert!(!saw_incoming(&a));
    let m = b
        .node
        .send_text(a.did.clone(), "still here?".into(), None)
        .unwrap();
    eventually(60, "message delivered while calls are not allowed", || {
        delivery(&b, &a.did, &m.id) == DeliveryState::Delivered
    });
    let call = c.node.call(a.did.clone()).unwrap();
    assert_eq!(incoming_within(&a, 30).peer_did, c.did);
    c.node.hangup(call.call_id.clone()).unwrap();
    ended_within(&c, &call.call_id, 30);

    a.node.set_available(false, None).unwrap();
    let call = c.node.call(a.did.clone()).unwrap();
    assert_eq!(ended_within(&c, &call.call_id, 40), "unreachable");
    assert!(c.logged("rejected: not accepted"));
}
