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
//!   merged between own installs (`crates/core/src/sync/accdoc.rs:241`, `:400`), but no code path
//!   adds to it. Blocking is per DID (`State.blocked`, `Node::remove_contact` `node.rs:984`).
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
//! bounded. No production code is changed; helpers are copied from `block_characterization.rs` /
//! `device_list_characterization.rs` (they are not in a shared module).

use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use identity::Identity;
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey};
use p2pcore::{CallInfo, CallState, DeliveryState, Message, Node, NodeEvents, NodeStatus};
use proto::Msg;
use serde::{Deserialize, Serialize};

const PASS: &str = "test-passphrase";
/// `GRANT_TTL` (`crates/core/src/node.rs:33`), private there.
const GRANT_TTL: u64 = 365 * 24 * 3600;
/// `CHAT_ALPN` (`crates/core/src/chat/wire.rs:9`), private there.
const CHAT_ALPN: &[u8] = b"tinline/chat/1";
const WAIT: Duration = Duration::from_secs(30);

// ---- helpers copied from block_characterization.rs / device_list_characterization.rs ----

#[derive(Debug, Clone)]
enum Ev {
    Incoming(CallInfo),
    State(String, CallState),
}

struct Tap(Mutex<mpsc::Sender<Ev>>, Arc<Mutex<Vec<String>>>);
impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.0.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.0.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, line: String) {
        self.1.lock().unwrap().push(line);
    }
}

struct Peer {
    node: Arc<Node>,
    rx: mpsc::Receiver<Ev>,
    logs: Arc<Mutex<Vec<String>>>,
    did: String,
    dir: std::path::PathBuf,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Peer {
    fn logged(&self, needle: &str) -> bool {
        self.logs.lock().unwrap().iter().any(|l| l.contains(needle))
    }
    fn dump(&self) -> String {
        format!(
            "{:?} {:?}",
            self.node.contacts(),
            self.node.blocked_for_test()
        )
    }
}

fn online(n: &Node) {
    n.start().unwrap();
    let t = Instant::now();
    while !n.status().online && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn peer(name: &str) -> Peer {
    let dir = std::env::temp_dir().join(format!("p2pcore-w2c-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let node = Node::new(
        dir.to_string_lossy().into(),
        Arc::new(Tap(Mutex::new(tx), logs.clone())),
    )
    .unwrap();
    node.create_identity(name.into(), PASS.into()).unwrap();
    online(&node);
    let did = node.profile().unwrap().did;
    Peer {
        node,
        rx,
        logs,
        did,
        dir,
    }
}

/// a and b, each a contact of the other (b scanned a's ticket).
fn pair(tag: &str) -> (Peer, Peer) {
    let a = peer(&format!("{tag}-a"));
    let b = peer(&format!("{tag}-b"));
    b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
    (a, b)
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

fn saw_incoming(p: &Peer) -> bool {
    p.rx.try_iter().any(|e| matches!(e, Ev::Incoming(_)))
}

fn all_msgs(p: &Peer, did: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut page = p.node.chat_day(did.to_string(), None).ok();
    while let Some(pg) = page {
        out.extend(pg.messages.clone());
        page = pg
            .older_day
            .and_then(|d| p.node.chat_day(did.to_string(), Some(d)).ok());
    }
    out
}

fn delivery(p: &Peer, did: &str, id: &str) -> DeliveryState {
    all_msgs(p, did)
        .into_iter()
        .find(|m| m.id == id)
        .expect("message held")
        .delivery
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("not within {secs}s: {what}");
}

// ---- end of copied helpers.

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The chat `Auth` frame, mirrored: variant 0 of `ChatMsg` (`crates/core/src/chat/wire.rs:30`),
/// bincode, u32 BE length prefix. Only the first frame of a session is read, which is `Auth`.
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
    /// The node's grant to the raw peer (what it presents to call / chat the node).
    from_node: proto::SignedGrant,
    /// The raw peer's grant to the node.
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
        let device = proto::device_public(&secret);
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
        proto::attest(&self.id, self.device, now())
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
        let device = proto::device_from_text(&ticket.device).unwrap();
        let mut addr = EndpointAddr::new(PublicKey::from_bytes(&device).unwrap());
        if let Some(url) = ticket
            .relay
            .as_deref()
            .and_then(|r| r.parse::<RelayUrl>().ok())
        {
            addr = addr.with_relay_url(url);
        }
        let from_node = self.rt.block_on(async {
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
            new.grant_from_them
        });
        Added {
            from_node,
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
                proto::call_hello(&self.id, self.attestation(), grant.clone(), call_id.into());
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
            let body = bincode::serialize(&auth).unwrap();
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
                            } = bincode::deserialize(&buf[4..4 + len]).ok()?;
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
    proto::verify_grant(g, &node.did, holder, now(), &HashSet::new()).expect("grant verifies")
}

fn is_reject(m: &Option<Msg>) -> bool {
    matches!(m, Some(Msg::Reject { reason }) if reason == "not accepted")
}

// ---- tests ----

// KIND: limitation
// TODAY a ticket contact receives exactly one credential: a DID-signed grant with cap "call" and
// a one-year TTL; there is no messaging capability at all. EXPECTED-CORRECT (U13): a ticket
// contact holds `message` and `call` tokens (or this legacy grant, read as message+call by the
// U13 migration rule). The proto model covers both shapes (`capability_tokens.rs`).
#[test]
fn ticket_contact_holds_one_call_grant_with_a_one_year_ttl() {
    let a = peer("ttl-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    let g = verify_from(&a, &raw.did(), &added.from_node);
    assert_eq!(g.cap, "call");
    assert_eq!(g.exp - g.iat, GRANT_TTL);
    assert_eq!(
        (g.iss.as_str(), g.holder.as_str()),
        (a.did.as_str(), raw.did().as_str())
    );
    assert!(a.node.contacts().iter().any(|c| c.did == raw.did()));
}

// KIND: limitation
// TODAY messaging is gated by the SAME "call" grant as calling: the grant that rings the node
// also opens a chat session (the node answers with its own `Auth`, presenting the grant WE issued
// it — mutual, and not a renewal), while a self-issued grant opens neither. There is no way to
// admit a contact to chat but not to calls. EXPECTED-CORRECT (U13): chat checks a `message`
// token and calls check a `call` token, so a message-only contact chats but cannot ring.
#[test]
fn one_call_grant_gates_both_calls_and_chat() {
    let a = peer("both-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());

    // Call path: rings.
    let (call, first) = raw.call(&added.addr, &added.from_node, "grantcall1");
    assert!(matches!(first, Some(Msg::Ringing)), "rings: {first:?}");
    assert_eq!(incoming_within(&a, 10).peer_did, raw.did());
    raw.hang_up(call);

    // Chat path: the same grant is accepted.
    let (att, grant_back) = raw
        .chat_auth(&added.addr, &added.from_node)
        .expect("chat accepted");
    let att = proto::verify_attestation(&att).unwrap();
    assert_eq!(att.did, a.did);
    assert_eq!(
        grant_back, added.to_node,
        "the node presents our grant, it issues nothing"
    );

    // A grant raw issued itself (wrong issuer) opens neither.
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
    let a = peer("lift-a");
    let b = Raw::new();
    let c = Raw::new();
    let b_added = b.add(&a.node.my_ticket().unwrap());
    let c_added = c.add(&a.node.my_ticket().unwrap());
    let before = a.dump();
    let (_, first) = c.call(&c_added.addr, &b_added.from_node, "liftcall1");
    assert!(is_reject(&first), "{first:?}");
    assert!(c.chat_auth(&c_added.addr, &b_added.from_node).is_none());
    assert!(a.logged("rejected call: holder is not who was expected"));
    assert!(!saw_incoming(&a));
    assert_eq!(a.dump(), before);
}

// KIND: candidate-policy G-RENEWAL
// TODAY's renewal rule: when the callee answers, its `Accept` carries a fresh "call" grant for the
// caller (new id, `iat` = now, one-year TTL). A declined call carries none; chat sessions never
// renew (previous test). Whether U13's call token keeps this rule, and how message tokens renew,
// is open (proto model: `call_token_renews_when_a_call_is_answered`, `message_token_*`).
#[test]
fn answering_renews_the_callers_grant_declining_does_not() {
    let a = peer("renew-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    let old = verify_from(&a, &raw.did(), &added.from_node);

    let (mut call, first) = raw.call(&added.addr, &added.from_node, "renewcall1");
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
    assert_eq!(new.cap, "call");
    assert_ne!(new.id, old.id, "a renewal is a new grant id");
    assert!(new.iat >= t_answer && new.iat >= old.iat);
    assert_eq!(new.exp - new.iat, GRANT_TTL);
    raw.hang_up(call);

    let (mut call, first) = raw.call(&added.addr, &added.from_node, "renewcall2");
    assert!(matches!(first, Some(Msg::Ringing)), "{first:?}");
    let info = incoming_within(&a, 10);
    a.node.decline(info.call_id).unwrap();
    let reply = raw.next(&mut call);
    assert!(matches!(reply, Some(Msg::Decline { .. })), "{reply:?}");
    raw.hang_up(call);
}

// KIND: limitation
// TODAY a block refuses by DID, not by grant: after `remove_contact` the contact's grant still
// verifies cryptographically and is not revoked, yet both paths refuse it with the generic
// "not accepted" (no signed "blocked" notice). EXPECTED-CORRECT (U13 + U5 revised): blocking
// revokes both tokens and the blocked party gets one DID-signed visible notice, then the same
// reason on every later attempt.
#[test]
fn block_refuses_by_did_with_the_generic_reason() {
    let a = peer("blk-a");
    let raw = Raw::new();
    let added = raw.add(&a.node.my_ticket().unwrap());
    a.node.remove_contact(raw.did()).unwrap();
    let before = a.dump();
    // The grant itself is still good: no revocation happened.
    verify_from(&a, &raw.did(), &added.from_node);
    for id in ["blkcall1", "blkcall2"] {
        let (_, first) = raw.call(&added.addr, &added.from_node, id);
        assert!(is_reject(&first), "{first:?}");
    }
    assert!(raw.chat_auth(&added.addr, &added.from_node).is_none());
    assert!(a.logged("rejected call: caller is not a contact"));
    assert!(!saw_incoming(&a));
    assert_eq!(a.dump(), before);
}

// KIND: limitation
// TODAY nothing adds to the grant revocation set: `State.revoked` is read on the call and chat
// paths and copied between own installs, but no production code inserts into it, so the only
// way to stop a contact's grant is the per-DID block. (Source scan of every non-test file in
// `crates/core/src`; a new file is not covered until listed here.) EXPECTED-CORRECT (U13):
// "disallow calls" and blocking revoke the specific tokens.
#[test]
fn no_code_path_writes_the_grant_revocation_set() {
    let files: &[(&str, &str)] = &[
        ("accounts.rs", include_str!("../src/accounts.rs")),
        ("chat/api.rs", include_str!("../src/chat/api.rs")),
        ("chat/blobs.rs", include_str!("../src/chat/blobs.rs")),
        ("chat/crypt.rs", include_str!("../src/chat/crypt.rs")),
        ("chat/doc.rs", include_str!("../src/chat/doc.rs")),
        ("chat/engine.rs", include_str!("../src/chat/engine.rs")),
        ("chat/mod.rs", include_str!("../src/chat/mod.rs")),
        ("chat/own.rs", include_str!("../src/chat/own.rs")),
        ("chat/presence.rs", include_str!("../src/chat/presence.rs")),
        ("chat/store.rs", include_str!("../src/chat/store.rs")),
        ("chat/wire.rs", include_str!("../src/chat/wire.rs")),
        ("error.rs", include_str!("../src/error.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
        ("logging.rs", include_str!("../src/logging.rs")),
        ("node.rs", include_str!("../src/node.rs")),
        ("rekey.rs", include_str!("../src/rekey.rs")),
        ("store.rs", include_str!("../src/store.rs")),
        ("sync/accdoc.rs", include_str!("../src/sync/accdoc.rs")),
        ("sync/api.rs", include_str!("../src/sync/api.rs")),
        ("sync/engine.rs", include_str!("../src/sync/engine.rs")),
        ("sync/frames.rs", include_str!("../src/sync/frames.rs")),
        ("sync/link.rs", include_str!("../src/sync/link.rs")),
        ("sync/mod.rs", include_str!("../src/sync/mod.rs")),
        ("vault.rs", include_str!("../src/vault.rs")),
        ("voice.rs", include_str!("../src/voice.rs")),
        ("wire.rs", include_str!("../src/wire.rs")),
    ];
    let mut uses = Vec::new();
    for (name, text) in files {
        for line in text.lines().map(str::trim) {
            if line.contains("revoked") && !line.starts_with("//") {
                uses.push((*name, line));
            }
        }
    }
    let writes: Vec<_> = uses
        .iter()
        .filter(|(_, l)| {
            [
                "revoked.insert",
                "revoked.extend",
                "revoked.remove",
                "revoked.retain",
                "revoked =",
            ]
            .iter()
            .any(|w| l.contains(w))
        })
        .collect();
    assert!(
        writes.is_empty(),
        "something revokes grants now: {writes:?}"
    );
    // The one mutable borrow is the account-doc merge from sibling installs.
    let muts: Vec<_> = uses
        .iter()
        .filter(|(_, l)| l.contains("&mut st.revoked"))
        .collect();
    assert_eq!(muts.len(), 1, "{muts:?}");
    assert_eq!(muts[0].0, "sync/accdoc.rs");
    // Read on both admission paths.
    assert!(
        uses.iter()
            .any(|(f, l)| *f == "node.rs" && l.contains("&s.state.revoked"))
    );
    assert!(
        uses.iter()
            .any(|(f, l)| *f == "chat/engine.rs" && l.contains("&s.state.revoked"))
    );
}

// KIND: invariant
// U13 default for ticket contacts already holds today: either side of a ticket-made pair can both
// message and call the other.
#[test]
fn ticket_contacts_can_message_and_call_each_other() {
    let (a, b) = pair("dflt");
    for (from, to) in [(&b, &a), (&a, &b)] {
        let m = from
            .node
            .send_text(to.did.clone(), "hi".into(), None)
            .unwrap();
        eventually(60, "message delivered", || {
            delivery(from, &to.did, &m.id) == DeliveryState::Delivered
        });
        let call = from.node.call(to.did.clone()).unwrap();
        assert_eq!(incoming_within(to, 30).peer_did, from.did);
        from.node.hangup(call.call_id.clone()).unwrap();
        ended_within(from, &call.call_id, 30);
    }
}

// KIND: limitation
// TODAY the only switch that refuses calls but keeps messages is the GLOBAL availability ("not
// now"): calls from every contact are refused with the generic reason and never ring, while
// messages are still delivered. There is no per-contact message-only state. EXPECTED-CORRECT
// (U13): per contact, a message-only contact is refused calls with a visible reason ("calls not
// allowed"), while other contacts can still ring.
#[test]
fn only_global_availability_separates_calls_from_messages() {
    let (a, b) = pair("away");
    a.node.set_available(false, None).unwrap();
    let call = b.node.call(a.did.clone()).unwrap();
    assert_eq!(ended_within(&b, &call.call_id, 40), "unreachable");
    assert!(!saw_incoming(&a));
    assert!(b.logged("rejected: not accepted"));
    let m = b
        .node
        .send_text(a.did.clone(), "still here?".into(), None)
        .unwrap();
    eventually(60, "message delivered while unavailable", || {
        delivery(&b, &a.did, &m.id) == DeliveryState::Delivered
    });
}
