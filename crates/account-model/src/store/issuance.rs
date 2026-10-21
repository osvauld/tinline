//! The account's [`proto::IssuanceStore`]s: what we remember about the tokens and tickets we
//! handed out. Both read `State` where it already lives: the token cut-offs are synced to our
//! other devices through the account doc, the tickets are device-local (a ticket names this
//! device). The admission rule itself is `proto::Issued::admit`.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cryptography::domains::Domain;
use proto::{Cap, IssuanceStore};

use super::State;

/// Our cut-offs for the tokens of one capability. Tokens are reusable until they expire, so none
/// is ever spent; a block raises both cut-offs, "disallow calls" the call one.
pub struct TokenCutOffs<'a> {
    pub st: &'a State,
    pub cap: Cap,
}

impl IssuanceStore for TokenCutOffs<'_> {
    fn spent(&self, _: &str, _: &[u8], _: &str) -> bool {
        false
    }

    fn cut_off_ms(&self, holder: &str, _: Domain) -> u64 {
        let c = self.st.cut_offs.get(holder).copied().unwrap_or_default();
        match self.cap {
            Cap::Message => c.message,
            Cap::Call => c.call,
        }
    }
}

/// Our contact tickets at unix second `now`. A ticket nonce still adds `holder` while it is a
/// one-time request ticket for exactly that DID (unexpired, unused: it is dropped when used), or
/// otherwise while it is a ticket we still hand out (`ticket_live`): the current one, or an older
/// one replaced but not reset. Shared codes are reusable by anyone until then (2026-10-14).
pub struct Tickets<'a> {
    pub st: &'a State,
    pub now: u64,
}

impl IssuanceStore for Tickets<'_> {
    fn spent(&self, _: &str, nonce: &[u8], holder: &str) -> bool {
        let n = URL_SAFE_NO_PAD.encode(nonce);
        match self.st.req_tickets.get(&n) {
            Some(t) => !(t.did == holder && t.exp >= self.now),
            None => !crate::tickets::ticket_live(self.st, &n, self.now),
        }
    }

    fn cut_off_ms(&self, _: &str, _: Domain) -> u64 {
        0
    }
}

#[cfg(test)]
mod tests;
