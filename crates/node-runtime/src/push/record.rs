//! A relay for p2pcore's integration tests (`testkit`): it seals nothing and sends nothing, it
//! hands out a made-up token per tag and records every wake asked for.

use super::*;
use wake_relay_core::api::IssuedToken;

#[derive(Default)]
pub struct RecordingRelay {
    wakes: Mutex<Vec<(String, Kind)>>,
}

impl RecordingRelay {
    /// How many `msg` wakes were asked for.
    pub fn msg_wakes(&self) -> usize {
        self.wakes
            .lock()
            .iter()
            .filter(|(_, k)| *k == Kind::Msg)
            .count()
    }
}

impl WakeRelay for RecordingRelay {
    fn register(&self, req: RegisterRequest) -> BoxFuture<Result<RegisterResponse, RelayError>> {
        let tokens = req
            .tags
            .iter()
            .map(|t| IssuedToken {
                tag: t.tag,
                token: wake::token_text(&t.tag),
            })
            .collect();
        Box::pin(async move { Ok(RegisterResponse { tokens }) })
    }

    fn wake(&self, token: String, kind: Kind) -> BoxFuture<u16> {
        self.wakes.lock().push((token, kind));
        Box::pin(async { 202 })
    }

    fn revoke(&self, _: String) -> BoxFuture<Result<(), RelayError>> {
        Box::pin(async { Ok(()) })
    }
}
