//! Which path the call's connection uses (iroh's selected path): direct on the LAN, direct over the
//! internet, or through a relay, with the remote address's type (never the address itself). The
//! classification is here; reading the selected path from the connection is p2pcore's `watch`.
use std::net::IpAddr;

/// (kind, address type) of an IP address: "lan" / "internet", and e.g. "IPv4 private".
pub fn classify_ip(ip: IpAddr) -> (&'static str, &'static str) {
    match ip {
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
                classify_ip(IpAddr::V4(v4))
            } else if (s[0] & 0xffc0) == 0xfe80 {
                ("lan", "IPv6 link-local")
            } else if (s[0] & 0xfe00) == 0xfc00 {
                ("lan", "IPv6 unique local")
            } else {
                ("internet", "IPv6 global")
            }
        }
    }
}

/// "Direct LAN", "Direct internet", "Relay".
pub fn kind_name(kind: &str) -> &'static str {
    match kind {
        "lan" => "Direct LAN",
        "internet" => "Direct internet",
        "relay" => "Relay",
        "other" => "Other transport",
        _ => "",
    }
}

#[cfg(test)]
mod tests;
