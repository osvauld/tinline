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

/// 1:1 chat (`tinline/chat/7`; 1 = before P9c, 2 = before P10's per-writer versions and P6's
/// message tokens; 3 = before profiles; 4 = before signed photo sizes and previews; 5 = before
/// recipient-signed delivered/read receipt frames; 6 = bincode frames, before postcard).
pub const CHAT: Versioned = Versioned { name: "chat", current: crate::chat::wire::CHAT_ALPN, old: &[b"tinline/chat/1", b"tinline/chat/2", b"tinline/chat/3", b"tinline/chat/4", b"tinline/chat/5", b"tinline/chat/6"] };
/// Own-device sync, which relays chat docs and the account doc (`tinline/self/5`; 1 = before
/// P9c, 2 = before P10 and P6's call tokens, call permissions and cut-offs; 3 = before signed photo sizes, which
/// an older own device would reject when it relays a /5 photo message; 4 = bincode frames).
pub const OWN: Versioned = Versioned { name: "own-device sync", current: proto::SELF_ALPN, old: &[b"tinline/self/1", b"tinline/self/2", b"tinline/self/3", b"tinline/self/4"] };
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
    // Profiles (chat/4), photo sizes (chat/5, self/4) and receipts (chat/6) only add to what is
    // stored: updating is enough. Postcard (chat/7, self/5) changed every stored format: wipe.
    let wipe = if matches!(alpn, b"tinline/chat/3" | b"tinline/chat/4" | b"tinline/chat/5" | b"tinline/self/3") {
        ""
    } else {
        " (and wipe its data)"
    };
    format!(
        "{} {} is no longer supported, this app speaks {}: update the app{}",
        v.name,
        String::from_utf8_lossy(alpn),
        String::from_utf8_lossy(v.current),
        wipe,
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
mod tests;
