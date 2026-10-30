//! Own devices hand each other their wake tokens (`docs/push.md` "Own devices"): one
//! `SelfMsg::OwnWake` per session once both Hellos carried [`CAPABILITY`], and again whenever
//! the token for that device changes. The token logic is node-runtime's `push/own.rs`.

use super::*;

/// The Hello marker: this device understands `SelfMsg::OwnWake`. An older device never sends it,
/// so it never gets the (to it unknown) message.
pub const CAPABILITY: &str = "cap/own-wake/v1";

/// What this session has told the peer about our wake token for it.
#[derive(Default)]
pub struct OwnWake {
    /// Both Hellos carried the marker.
    pub enabled: bool,
    /// The last value sent (`None`: nothing sent yet).
    sent: Option<Option<(String, u64)>>,
}

/// Whether the peer's Hello carries the marker (an empty version vector, like the other caps).
pub fn supported(docs: &[(String, Vec<u8>)]) -> bool {
    docs.iter().any(|(name, bytes)| {
        name == CAPABILITY
            && VersionVector::decode(bytes)
                .ok()
                .is_some_and(|v| v == VersionVector::default())
    })
}

impl OwnWake {
    /// The message to send now, if our token for the peer differs from what it was last told.
    pub fn due(&mut self, ours: Option<(String, u64)>) -> Option<SelfMsg> {
        if !self.enabled || self.sent.as_ref() == Some(&ours) {
            return None;
        }
        self.sent = Some(ours.clone());
        Some(match ours {
            Some((token, exp_ms)) => SelfMsg::OwnWake {
                token: Some(token),
                exp_ms,
            },
            None => SelfMsg::OwnWake {
                token: None,
                exp_ms: 0,
            },
        })
    }
}

impl SyncRt {
    /// Sends our wake token for `remote` if this session has not told it yet.
    pub async fn push_own_wake(
        &self,
        remote: &[u8; 32],
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if let Some(m) = peer.own_wake.due(self.own_wake_token_for(remote)) {
            writer.send(&m).await?;
        }
        Ok(())
    }

    /// The peer's `OwnWake`: kept (or dropped) by node-runtime, only from a live own device
    /// (`WakeState::hold` refuses an oversized, expired or implausibly long-lived token).
    pub fn own_wake_in(&self, remote: [u8; 32], token: Option<String>, exp_ms: u64) {
        self.ctx
            .own_wake_token_in(remote, token.map(|t| (t, exp_ms)));
    }

    /// Our sync dial of own device `device` failed while it is owed a change since `generation`
    /// (`SelfSync::held` below it): wake it, once per the `msg` throttle. Not at a bare start
    /// (generation 0 and nothing known), so restarting a computer does not wake the phone; not
    /// after a kick alone (`held` outlives it); not while a session with it is up.
    pub async fn wake_owed_device(self: &Arc<Self>, device: [u8; 32], generation: u64) -> bool {
        if self.selfsync.sessions.lock().contains_key(&device) {
            return false;
        }
        let known = self.selfsync.held.lock().get(&device).copied().unwrap_or(0);
        if generation <= known || !self.ctx.can_wake_own(&device) {
            return false;
        }
        self.ctx.wake_own_device(device).await
    }
}

#[cfg(test)]
mod tests;
