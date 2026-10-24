use super::*;

#[test]
fn paths_are_named_by_kind_and_address_type() {
    let ip = |s: &str| classify_ip(s.parse::<std::net::SocketAddr>().unwrap().ip());
    assert_eq!(ip("192.168.1.7:4433"), ("lan", "IPv4 private"));
    assert_eq!(ip("10.0.0.2:1"), ("lan", "IPv4 private"));
    assert_eq!(ip("100.101.1.2:1"), ("internet", "IPv4 shared (CGNAT)"));
    assert_eq!(ip("8.8.8.8:1"), ("internet", "IPv4 public"));
    assert_eq!(ip("[fe80::1]:1"), ("lan", "IPv6 link-local"));
    assert_eq!(ip("[fd00::1]:1"), ("lan", "IPv6 unique local"));
    assert_eq!(ip("[2001:db8::1]:1"), ("internet", "IPv6 global"));
    assert_eq!(ip("[::ffff:192.168.0.3]:1"), ("lan", "IPv4 private"));
    assert_eq!(kind_name("lan"), "Direct LAN");
}
