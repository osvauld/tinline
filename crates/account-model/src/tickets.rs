//! Which of our contact tickets still add contacts. p2pcore owns minting and redeeming them.

use proto::ContactTicket;

use crate::store::State;

/// Whether our ticket with this nonce still adds contacts: the current one, or an older one
/// that was replaced but not reset, until it expires.
pub fn ticket_live(st: &State, nonce: &str, now: u64) -> bool {
    st.live_tickets.get(nonce).is_some_and(|&exp| exp > now) || current_nonce(st, now).as_deref() == Some(nonce)
}

/// The nonce of the ticket we currently hand out, if it is still valid.
pub fn current_nonce(state: &State, now: u64) -> Option<String> {
    let t = ContactTicket::from_text(state.ticket.as_deref()?).ok()?;
    Some(t.verify(now).ok()?.nonce)
}
