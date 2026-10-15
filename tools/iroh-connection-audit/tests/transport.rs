//! Local-only transport characterization, NOT app integration/capacity testing.
//! No relays, address lookup, real identities, group authority or phone operations.
use std::time::Duration;

use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, SecretKey};
use tokio::time::timeout;

const GROUP: &[u8] = b"audit/group";
const OTHER: &[u8] = b"audit/other";
const CASE_BUDGET: Duration = Duration::from_secs(20);

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .clear_ip_transports()
        .clear_relay_transports()
        .clear_address_lookup()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .alpns(vec![GROUP.to_vec(), OTHER.to_vec()])
        .bind()
        .await
        .unwrap()
}

async fn connect_pair(
    client: &Endpoint,
    server: &Endpoint,
    alpn: &[u8],
) -> (Connection, Connection) {
    // Explicit local address hints: no discovery or public infrastructure required.
    let (outgoing, incoming) = tokio::join!(client.connect(server.addr(), alpn), async {
        server.accept().await.expect("listener open").await
    });
    (outgoing.unwrap(), incoming.unwrap())
}

async fn close_pair(client: Connection, server: Connection) {
    client.close(0u32.into(), b"audit complete");
    timeout(Duration::from_secs(3), server.closed())
        .await
        .expect("remote observed close");
    assert!(client.close_reason().is_some());
    assert!(server.close_reason().is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_connect_same_endpoints_creates_distinct_connections() {
    timeout(CASE_BUDGET, async {
        let (client, server) = (endpoint().await, endpoint().await);
        let (c1, s1) = connect_pair(&client, &server, GROUP).await;
        let (c2, s2) = connect_pair(&client, &server, GROUP).await;
        assert_ne!(c1.stable_id(), c2.stable_id());
        assert_ne!(s1.stable_id(), s2.stable_id());
        assert_eq!(c1.remote_id(), c2.remote_id());
        eprintln!("same endpoint/ALPN: 2 distinct QUIC connections per side");
        close_pair(c1, s1).await;
        close_pair(c2, s2).await;
        client.close().await;
        server.close().await;
    })
    .await
    .expect("bounded characterization case");
}

#[tokio::test(flavor = "current_thread")]
async fn two_group_streams_can_share_one_connection_when_application_supports_it() {
    timeout(CASE_BUDGET, async {
        let (client, server) = (endpoint().await, endpoint().await);
        let (c, s) = connect_pair(&client, &server, GROUP).await;
        let stable_id = c.stable_id();
        // Keep both streams open together. Scope authorization is NOT supplied by
        // the transport; production needs separate admission for each group.
        let server_work = async {
            let mut streams = Vec::new();
            for _ in 0..2 {
                streams.push(s.accept_bi().await.unwrap());
            }
            for (mut send, mut recv) in streams {
                let request = recv.read_to_end(64).await.unwrap();
                send.write_all(&request).await.unwrap();
                send.finish().unwrap();
            }
        };
        let client_work = async {
            let mut streams = Vec::new();
            for payload in [b"group:a".as_slice(), b"group:b".as_slice()] {
                let (mut send, recv) = c.open_bi().await.unwrap();
                send.write_all(payload).await.unwrap();
                send.finish().unwrap();
                streams.push((recv, payload));
            }
            for (mut recv, expected) in streams {
                assert_eq!(recv.read_to_end(64).await.unwrap(), expected);
            }
        };
        tokio::join!(server_work, client_work);
        assert_eq!(c.stable_id(), stable_id);
        assert_eq!(c.remote_id(), server.id());
        eprintln!(
            "shared transport: 1 QUIC connection, 2 bidirectional streams; stats={:?}",
            c.stats()
        );
        close_pair(c, s).await;
        client.close().await;
        server.close().await;
    })
    .await
    .expect("bounded characterization case");
}

#[tokio::test(flavor = "current_thread")]
async fn different_alpns_use_distinct_connections() {
    timeout(CASE_BUDGET, async {
        let (client, server) = (endpoint().await, endpoint().await);
        let (c1, s1) = connect_pair(&client, &server, GROUP).await;
        let (c2, s2) = connect_pair(&client, &server, OTHER).await;
        assert_ne!(c1.stable_id(), c2.stable_id());
        assert_eq!(c1.alpn(), GROUP);
        assert_eq!(c2.alpn(), OTHER);
        assert_eq!(s1.alpn(), GROUP);
        assert_eq!(s2.alpn(), OTHER);
        eprintln!("same endpoint, 2 ALPNs: 2 distinct QUIC connections per side");
        close_pair(c1, s1).await;
        close_pair(c2, s2).await;
        client.close().await;
        server.close().await;
    })
    .await
    .expect("bounded characterization case");
}

#[tokio::test(flavor = "current_thread")]
async fn resetting_one_group_stream_does_not_close_shared_transport() {
    timeout(CASE_BUDGET, async {
        let (client, server) = (endpoint().await, endpoint().await);
        let (c, s) = connect_pair(&client, &server, GROUP).await;
        let (client_stream, server_stream) = tokio::join!(
            async {
                let (mut send, recv) = c.open_bi().await.unwrap();
                send.write_all(b"group:a").await.unwrap();
                (send, recv)
            },
            s.accept_bi()
        );
        let (mut send, recv) = client_stream;
        let (server_send, mut server_recv) = server_stream.unwrap();
        send.reset(1u32.into()).unwrap();
        drop(recv);
        assert!(server_recv.read_to_end(64).await.is_err());
        drop(server_send);
        assert!(c.close_reason().is_none() && s.close_reason().is_none());
        let client_work = async {
            let (mut send, mut recv) = c.open_bi().await.unwrap();
            send.write_all(b"group:b").await.unwrap();
            send.finish().unwrap();
            assert_eq!(recv.read_to_end(64).await.unwrap(), b"group:b");
        };
        let server_work = async {
            let (mut send, mut recv) = s.accept_bi().await.unwrap();
            let payload = recv.read_to_end(64).await.unwrap();
            send.write_all(&payload).await.unwrap();
            send.finish().unwrap();
        };
        tokio::join!(client_work, server_work);
        eprintln!("one group stream reset; another stream remains usable on same transport");
        close_pair(c, s).await;
        client.close().await;
        server.close().await;
    })
    .await
    .expect("bounded characterization case");
}
