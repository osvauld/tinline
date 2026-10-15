//! Small inventory smoke cases, not a maximum-capacity or Android benchmark.
use std::collections::HashSet;
use std::time::{Duration, Instant};

use iroh::endpoint::{QuicTransportConfig, presets};
use iroh::{Endpoint, SecretKey};
use tokio::time::{sleep, timeout};

const ALPN: &[u8] = b"audit/inventory";

async fn endpoint() -> Endpoint {
    // Match the inspected application's group keepalive/idle settings only;
    // all public discovery/relay services remain disabled.
    let config = QuicTransportConfig::builder()
        .keep_alive_interval(Duration::from_secs(5))
        .max_idle_timeout(Some(Duration::from_secs(20).try_into().unwrap()))
        .build();
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .clear_ip_transports()
        .clear_relay_transports()
        .clear_address_lookup()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .alpns(vec![ALPN.to_vec()])
        .transport_config(config)
        .bind()
        .await
        .unwrap()
}

fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmRSS:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

#[tokio::test(flavor = "current_thread")]
async fn inventory_one_four_sixteen_connections_remains_usable_and_closes() {
    timeout(Duration::from_secs(30), async {
        for count in [1, 4, 16] {
            let (client, server) = (endpoint().await, endpoint().await);
            let before_rss = rss_kib();
            let mut pairs = Vec::new();
            let started = Instant::now();
            // Serial dialing deliberately avoids a reconnect storm or CPU load.
            for _ in 0..count {
                let (outgoing, incoming) = tokio::join!(
                    client.connect(server.addr(), ALPN),
                    async { server.accept().await.unwrap().await }
                );
                pairs.push((outgoing.unwrap(), incoming.unwrap()));
            }
            let dial_elapsed = started.elapsed();
            assert_eq!(pairs.iter().map(|(c, _)| c.stable_id()).collect::<HashSet<_>>().len(), count);
            assert_eq!(pairs.iter().map(|(_, s)| s.stable_id()).collect::<HashSet<_>>().len(), count);
            assert!(pairs.iter().all(|(c, s)| c.close_reason().is_none() && s.close_reason().is_none()));
            let held_rss = rss_kib();
            let started = Instant::now();
            for (c, s) in &pairs {
                let client_work = async {
                    let (mut send, mut recv) = c.open_bi().await.unwrap();
                    send.write_all(b"still usable").await.unwrap();
                    send.finish().unwrap();
                    assert_eq!(recv.read_to_end(64).await.unwrap(), b"still usable");
                };
                let server_work = async {
                    let (mut send, mut recv) = s.accept_bi().await.unwrap();
                    let message = recv.read_to_end(64).await.unwrap();
                    send.write_all(&message).await.unwrap();
                    send.finish().unwrap();
                };
                tokio::join!(client_work, server_work);
            }
            let echo_elapsed = started.elapsed();
            sleep(Duration::from_millis(200)).await;
            let tx_bytes: u64 = pairs.iter().map(|(c, _)| c.stats().udp_tx.bytes).sum();
            eprintln!(
                "inventory count={count} dial_ms={} echo_ms={} process_rss_before_kib={before_rss:?} process_rss_held_kib={held_rss:?} client_total_tx_bytes={tx_bytes}",
                dial_elapsed.as_millis(), echo_elapsed.as_millis()
            );
            // RSS includes BOTH endpoints, TLS, runtime and allocator retention.
            // tx includes handshake/control/echo traffic, NOT an idle byte rate.
            for (c, s) in pairs {
                c.close(0u32.into(), b"inventory complete");
                timeout(Duration::from_secs(3), s.closed()).await.unwrap();
                assert!(c.close_reason().is_some() && s.close_reason().is_some());
            }
            client.close().await;
            server.close().await;
        }
    })
    .await
    .expect("bounded low-concurrency inventory case");
}
