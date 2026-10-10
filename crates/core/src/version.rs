//! Protocol versions on the wire (P9c, U18). Chat content changed incompatibly (DID-signed record
//! fields, writer IDs only; docs/chat.md "Signatures and writer IDs"), and so did what own-device
//! sync relays. An app that speaks an older version is refused with a clear reason, both ways,
//! instead of breaking silently:
//!
//! - Accepting: our endpoint still lists the old ALPNs, only so that an old app's connection is
//!   accepted and closed at once with `VERSION_MISMATCH` and a reason naming both versions
//!   (`node.rs` accept loop -> `refuse`).
//! - Dialing: we offer the old ALPNs as well (`connect`). If the peer picks one, it is an old app:
//!   we close with the same code and reason, and the dial fails with an error that says so.

use std::time::Duration;

use iroh::endpoint::{Accepting, ConnectOptions, Connection};
use iroh::{Endpoint, EndpointAddr};

use crate::Error;

/// QUIC application close code: the peer speaks a protocol version this app no longer supports.
pub const VERSION_MISMATCH: u32 = 426;

/// A protocol whose wire version was bumped incompatibly.
#[derive(Debug)]
pub struct Versioned {
    pub name: &'static str,
    pub current: &'static [u8],
    /// Versions this app refuses (clearly) instead of speaking.
    pub old: &'static [&'static [u8]],
}

/// 1:1 chat (`tinline/chat/3`; 1 = before P9c, 2 = before P10's per-writer versions and P6's
/// message tokens; one bump for both, same release).
pub const CHAT: Versioned = Versioned { name: "chat", current: crate::chat::wire::CHAT_ALPN, old: &[b"tinline/chat/1", b"tinline/chat/2"] };
/// Own-device sync, which relays chat docs and the account doc (`tinline/self/3`; 1 = before
/// P9c, 2 = before P10 and P6's call tokens, call permissions and cut-offs).
pub const OWN: Versioned = Versioned { name: "own-device sync", current: proto::SELF_ALPN, old: &[b"tinline/self/1", b"tinline/self/2"] };
/// Calls, contact exchange, notices and tokens (`osvauld/p2p/1`; 0 = one `call` grant for
/// everything, before P6).
pub const CALL: Versioned = Versioned { name: "calls and contacts", current: proto::ALPN, old: &[b"osvauld/p2p/0"] };

const ALL: [&Versioned; 3] = [&CHAT, &OWN, &CALL];

/// The ALPNs our endpoint lists only to refuse them.
pub fn old_alpns() -> Vec<Vec<u8>> {
    ALL.iter().flat_map(|v| v.old.iter().map(|a| a.to_vec())).collect()
}

/// The protocol `alpn` is a superseded version of, if it is one.
pub fn superseded(alpn: &[u8]) -> Option<&'static Versioned> {
    ALL.into_iter().find(|v| v.old.contains(&alpn))
}

/// The close reason an old peer gets (and the error a dial to one reports).
pub fn reason(v: &Versioned, alpn: &[u8]) -> String {
    format!(
        "{} {} is no longer supported, this app speaks {}: update the app (and wipe its data)",
        v.name,
        String::from_utf8_lossy(alpn),
        String::from_utf8_lossy(v.current)
    )
}

/// Accepts an old app's connection and closes it at once with `VERSION_MISMATCH` and `reason`.
pub async fn refuse(accepting: Accepting, v: &Versioned, alpn: &[u8]) -> Result<(), Error> {
    let conn = tokio::time::timeout(Duration::from_secs(15), accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
    conn.close(VERSION_MISMATCH.into(), reason(v, alpn).as_bytes());
    Ok(())
}

/// Dials `addr` on `v.current`, offering the old versions too so that an old app answers instead
/// of failing the handshake; such a connection is closed with `VERSION_MISMATCH` and the dial
/// fails with `reason`.
pub async fn connect(ep: &Endpoint, addr: EndpointAddr, v: &Versioned) -> Result<Connection, Error> {
    let opts = ConnectOptions::new().with_additional_alpns(v.old.iter().map(|a| a.to_vec()).collect());
    let conn = ep.connect_with_opts(addr, v.current, opts).await.map_err(Error::net)?.await.map_err(Error::net)?;
    let alpn = conn.alpn().to_vec();
    if alpn.as_slice() == v.current {
        return Ok(conn);
    }
    let why = reason(v, &alpn);
    conn.close(VERSION_MISMATCH.into(), why.as_bytes());
    Err(Error::Protocol(format!("the other device runs an older app: {why}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;
    use iroh::endpoint::{ConnectionError, presets};

    async fn endpoint(alpns: Vec<Vec<u8>>) -> Endpoint {
        Endpoint::builder(presets::Minimal).secret_key(SecretKey::generate()).alpns(alpns).bind().await.unwrap()
    }

    /// What our endpoint lists for a protocol: the current version and the old ones.
    fn ours(v: &Versioned) -> Vec<Vec<u8>> {
        let mut a = vec![v.current.to_vec()];
        a.extend(v.old.iter().map(|o| o.to_vec()));
        a
    }

    /// Accepts one connection the way the node's accept loop does: an old version is refused,
    /// the current one is kept open (returned).
    async fn serve_one(ep: Endpoint) -> Option<Connection> {
        let mut accepting = ep.accept().await.unwrap().accept().unwrap();
        let alpn = accepting.alpn().await.unwrap();
        if let Some(v) = superseded(&alpn) {
            refuse(accepting, v, &alpn).await.unwrap();
            return None;
        }
        Some(accepting.await.unwrap())
    }

    fn mismatch(e: ConnectionError) -> String {
        match e {
            ConnectionError::ApplicationClosed(c) => {
                assert_eq!(c.error_code, VERSION_MISMATCH.into());
                String::from_utf8_lossy(&c.reason).to_string()
            }
            other => panic!("not a version mismatch: {other:?}"),
        }
    }

    // KIND: invariant (P9c; P10 bumped again: a record holds one version per writer)
    #[test]
    fn the_wire_versions_were_bumped_and_the_old_ones_are_refused() {
        assert_eq!(CHAT.current, b"tinline/chat/3");
        assert_eq!(OWN.current, b"tinline/self/3");
        assert_eq!(CALL.current, b"osvauld/p2p/1");
        for v in ALL {
            assert!(superseded(v.current).is_none());
            for o in v.old {
                assert_ne!(*o, v.current);
                assert_eq!(superseded(o).map(|s| s.name), Some(v.name));
            }
        }
        assert_eq!(
            old_alpns(),
            vec![
                b"tinline/chat/1".to_vec(),
                b"tinline/chat/2".to_vec(),
                b"tinline/self/1".to_vec(),
                b"tinline/self/2".to_vec(),
                b"osvauld/p2p/0".to_vec()
            ]
        );
        let r = reason(&CHAT, b"tinline/chat/2");
        assert!(r.contains("tinline/chat/2") && r.contains("tinline/chat/3") && r.contains("update"), "{r}");
    }

    // KIND: invariant (P9c, test-first)
    // An old app (pre-P9c: it dials `tinline/chat/1` only) reaching us: accepted and closed at
    // once with VERSION_MISMATCH and a reason naming both versions. Same for own-device sync.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_old_app_dialing_us_gets_a_clear_version_mismatch() {
        for v in ALL {
            let new = endpoint(ours(v)).await;
            let addr = new.addr();
            let server = tokio::spawn(serve_one(new.clone()));
            let old = endpoint(vec![]).await;
            let conn = old.connect(addr, v.old[0]).await.unwrap();
            let why = mismatch(tokio::time::timeout(Duration::from_secs(10), conn.closed()).await.unwrap());
            assert!(why.contains(&String::from_utf8_lossy(v.old[0]).to_string()) && why.contains(&String::from_utf8_lossy(v.current).to_string()), "{why}");
            assert!(server.await.unwrap().is_none());
            old.close().await;
            new.close().await;
        }
    }

    // KIND: invariant (P9c, test-first)
    // Us dialing an old app (it lists only `tinline/chat/1`): the dial fails with an error that
    // names the old version, and the old app sees the connection closed with VERSION_MISMATCH.
    #[tokio::test(flavor = "multi_thread")]
    async fn dialing_an_old_app_fails_with_a_clear_error() {
        for v in ALL {
            let old = endpoint(vec![v.old[0].to_vec()]).await;
            let addr = old.addr();
            let server = tokio::spawn({
                let old = old.clone();
                async move { old.accept().await.unwrap().accept().unwrap().await.unwrap() }
            });
            let new = endpoint(vec![]).await;
            let err = connect(&new, addr, v).await.expect_err("refused");
            let text = err.to_string();
            assert!(text.contains("older app") && text.contains(&String::from_utf8_lossy(v.old[0]).to_string()), "{text}");
            let seen_by_old = server.await.unwrap();
            mismatch(tokio::time::timeout(Duration::from_secs(10), seen_by_old.closed()).await.unwrap());
            new.close().await;
            old.close().await;
        }
    }

    // KIND: invariant
    // Two current apps: the current version is picked and the connection stays up.
    #[tokio::test(flavor = "multi_thread")]
    async fn two_current_apps_speak_the_current_version() {
        let a = endpoint(ours(&CHAT)).await;
        let addr = a.addr();
        let server = tokio::spawn(serve_one(a.clone()));
        let b = endpoint(vec![]).await;
        let conn = connect(&b, addr, &CHAT).await.unwrap();
        assert_eq!(conn.alpn(), CHAT.current);
        let served = server.await.unwrap().expect("kept");
        assert_eq!(served.alpn(), CHAT.current);
        assert!(conn.close_reason().is_none());
        b.close().await;
        a.close().await;
    }
}
