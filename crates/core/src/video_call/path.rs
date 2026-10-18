//! Which path the call's connection uses (iroh's selected path): direct on the LAN, direct over the
//! internet, or through a relay, with the remote address's type (never the address itself) and
//! QUIC's measured RTT on it. Read once a second for the stats sheet and logged once per call and
//! on every change (`video path ...`), so a call log says what the rate control was running on.
use std::net::IpAddr;

use iroh::TransportAddr;

use super::*;

/// (kind, address type): "lan" / "internet" / "relay" / "other", and e.g. "IPv4 private".
pub(super) fn classify(addr: &TransportAddr) -> (&'static str, &'static str) {
    match addr {
        TransportAddr::Relay(_) => ("relay", "relay server"),
        TransportAddr::Ip(sa) => match sa.ip() {
            IpAddr::V4(v4) => {
                let o = v4.octets();
                if v4.is_loopback() {
                    ("lan", "IPv4 loopback")
                } else if v4.is_private() {
                    ("lan", "IPv4 private")
                } else if v4.is_link_local() {
                    ("lan", "IPv4 link-local")
                } else if o[0] == 100 && (o[1] & 0xc0) == 64 {
                    // 100.64/10: carrier-grade NAT (or an overlay network such as Tailscale).
                    ("internet", "IPv4 shared (CGNAT)")
                } else {
                    ("internet", "IPv4 public")
                }
            }
            IpAddr::V6(v6) => {
                let s = v6.segments();
                if v6.is_loopback() {
                    ("lan", "IPv6 loopback")
                } else if let Some(v4) = v6.to_ipv4_mapped() {
                    classify(&TransportAddr::Ip(std::net::SocketAddr::new(IpAddr::V4(v4), sa.port())))
                } else if (s[0] & 0xffc0) == 0xfe80 {
                    ("lan", "IPv6 link-local")
                } else if (s[0] & 0xfe00) == 0xfc00 {
                    ("lan", "IPv6 unique local")
                } else {
                    ("internet", "IPv6 global")
                }
            }
        },
        _ => ("other", "custom transport"),
    }
}

/// "Direct LAN", "Direct internet", "Relay".
pub(super) fn kind_name(kind: &str) -> &'static str {
    match kind {
        "lan" => "Direct LAN",
        "internet" => "Direct internet",
        "relay" => "Relay",
        "other" => "Other transport",
        _ => "",
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
    fn paths_are_named_by_kind_and_address_type() {
        let ip = |s: &str| TransportAddr::Ip(s.parse().unwrap());
        assert_eq!(classify(&ip("192.168.1.7:4433")), ("lan", "IPv4 private"));
        assert_eq!(classify(&ip("10.0.0.2:1")), ("lan", "IPv4 private"));
        assert_eq!(classify(&ip("100.101.1.2:1")), ("internet", "IPv4 shared (CGNAT)"));
        assert_eq!(classify(&ip("8.8.8.8:1")), ("internet", "IPv4 public"));
        assert_eq!(classify(&ip("[fe80::1]:1")), ("lan", "IPv6 link-local"));
        assert_eq!(classify(&ip("[fd00::1]:1")), ("lan", "IPv6 unique local"));
        assert_eq!(classify(&ip("[2001:db8::1]:1")), ("internet", "IPv6 global"));
        assert_eq!(classify(&ip("[::ffff:192.168.0.3]:1")), ("lan", "IPv4 private"));
        let relay = TransportAddr::Relay("https://relay.example.test".parse().unwrap());
        assert_eq!(classify(&relay), ("relay", "relay server"));
        assert_eq!(kind_name("lan"), "Direct LAN");
    }
}
