//! Which path the call's connection uses (iroh's selected path), read once a second for the stats
//! sheet and logged once per call and on every change (`video path ...`), so a call log says what
//! the rate control was running on. The classification is `video_call_core::path`.
use iroh::TransportAddr;
use video_call_core::path::{classify_ip, kind_name};

use super::*;

/// (kind, address type): "lan" / "internet" / "relay" / "other", and e.g. "IPv4 private".
fn classify(addr: &TransportAddr) -> (&'static str, &'static str) {
    match addr {
        TransportAddr::Relay(_) => ("relay", "relay server"),
        TransportAddr::Ip(sa) => classify_ip(sa.ip()),
        _ => ("other", "custom transport"),
    }
}

/// Once a second for the life of the call's video: the selected path into the stats, logged when
/// it changes (kind or address type; RTT alone is not a change).
pub(super) async fn watch(conn: Connection, pipe: Arc<Pipe>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last: Option<(&'static str, &'static str)> = None;
    loop {
        tick.tick().await;
        if pipe.stop.load(Ordering::SeqCst) {
            return;
        }
        let sel = conn.paths().iter().find(|p| p.is_selected()).map(|p| (classify(p.remote_addr()), p.rtt().as_millis() as u32));
        let Some(((kind, addr), rtt)) = sel else { continue };
        if last != Some((kind, addr)) {
            tracing::info!(target: "p2pcore::video_call", "video path {} ({addr}) rtt={rtt}ms{}", kind_name(kind), if last.is_some() { " (changed)" } else { "" });
            last = Some((kind, addr));
        }
        let mut st = pipe.stats.lock();
        (st.path_kind, st.path_addr, st.path_rtt_ms) = (kind.into(), addr.into(), rtt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_and_ip_paths_are_named() {
        let ip = |s: &str| TransportAddr::Ip(s.parse().unwrap());
        assert_eq!(classify(&ip("192.168.1.7:4433")), ("lan", "IPv4 private"));
        let relay = TransportAddr::Relay("https://relay.example.test".parse().unwrap());
        assert_eq!(classify(&relay), ("relay", "relay server"));
    }
}
