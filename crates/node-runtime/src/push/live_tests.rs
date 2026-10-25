//! A check against the deployed relay. Skipped unless `P2P_WAKE_LIVE=1`; it only calls the
//! health and register endpoints (harmless: the relay stores nothing per token).

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wake_live_relay_answers_over_the_hand_built_tls_config() {
    if std::env::var("P2P_WAKE_LIVE").as_deref() != Ok("1") {
        return;
    }
    let relay = HttpRelay::new(RELAY_URL);
    let health = relay.client().expect("the client builds").get(format!("{RELAY_URL}/v1/health")).send().await.expect("health request");
    assert_eq!(health.status().as_u16(), 200);

    let req = RegisterRequest {
        target: Target::UnifiedPush { endpoint: "https://example.com/up/x".into() },
        hint_key: [7; 32],
        tags: vec![wake_relay_core::api::TagRequest { tag: random_tag(), kinds: Kinds::MSG, exp_ms: now_ms() + 24 * 3600 * 1000 }],
    };
    let resp = relay.register(req).await.expect("register answers 2xx");
    assert_eq!(resp.tokens.len(), 1);
}
