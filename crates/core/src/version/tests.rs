use super::*;
use iroh::SecretKey;
use iroh::endpoint::{ConnectionError, presets};

async fn endpoint(alpns: Vec<Vec<u8>>) -> Endpoint {
    Endpoint::builder(presets::Minimal).secret_key(SecretKey::generate()).alpns(alpns).bind().await.unwrap()
}

/// What our endpoint lists for a protocol: the current version and the old ones.
fn ours(v: &Versioned) -> Vec<Vec<u8>> {
    let mut a = vec![v.current.to_vec()];
    a.extend(v.old.iter().map(|o| o.to_vec()));
    a
}

/// Accepts one connection the way the node's accept loop does: an old version is refused,
/// the current one is kept open (returned).
async fn serve_one(ep: Endpoint) -> Option<Connection> {
    let mut accepting = ep.accept().await.unwrap().accept().unwrap();
    let alpn = accepting.alpn().await.unwrap();
    if let Some(v) = superseded(&alpn) {
        refuse(accepting, v, &alpn).await.unwrap();
        return None;
    }
    Some(accepting.await.unwrap())
}

fn mismatch(e: ConnectionError) -> String {
    match e {
        ConnectionError::ApplicationClosed(c) => {
            assert_eq!(c.error_code, VERSION_MISMATCH.into());
            String::from_utf8_lossy(&c.reason).to_string()
        }
        other => panic!("not a version mismatch: {other:?}"),
    }
}

// KIND: invariant (P9c; P10 bumped again: a record holds one version per writer)
#[test]
fn the_wire_versions_were_bumped_and_the_old_ones_are_refused() {
    assert_eq!(CHAT.current, b"tinline/chat/7");
    assert!(reason(&CHAT, b"tinline/chat/6").contains("wipe"), "postcard changed every stored format");
    assert!(reason(&OWN, b"tinline/self/4").contains("wipe"));
    assert!(!reason(&CHAT, b"tinline/chat/5").contains("wipe"), "receipt frames do not require a data wipe");
    assert!(!reason(&CHAT, b"tinline/chat/3").contains("wipe"), "profile support upgrades do not require a data wipe");
    assert!(!reason(&CHAT, b"tinline/chat/4").contains("wipe"), "photo sizes do not require a data wipe");
    assert!(!reason(&OWN, b"tinline/self/3").contains("wipe"), "photo sizes do not require a data wipe");
    assert!(reason(&CHAT, b"tinline/chat/2").contains("wipe"));
    assert_eq!(OWN.current, b"tinline/self/5");
    assert_eq!(CALL.current, b"osvauld/p2p/1");
    for v in ALL {
        assert!(superseded(v.current).is_none());
        for o in v.old {
            assert_ne!(*o, v.current);
            assert_eq!(superseded(o).map(|s| s.name), Some(v.name));
        }
    }
    assert_eq!(
        old_alpns(),
        vec![
            b"tinline/chat/1".to_vec(),
            b"tinline/chat/2".to_vec(),
            b"tinline/chat/3".to_vec(),
            b"tinline/chat/4".to_vec(),
            b"tinline/chat/5".to_vec(),
            b"tinline/chat/6".to_vec(),
            b"tinline/self/1".to_vec(),
            b"tinline/self/2".to_vec(),
            b"tinline/self/3".to_vec(),
            b"tinline/self/4".to_vec(),
            b"osvauld/p2p/0".to_vec()
        ]
    );
    let r = reason(&CHAT, b"tinline/chat/2");
    assert!(r.contains("tinline/chat/2") && r.contains("tinline/chat/7") && r.contains("update"), "{r}");
}

// KIND: invariant (P9c, test-first)
// An old app (pre-P9c: it dials `tinline/chat/1` only) reaching us: accepted and closed at
// once with VERSION_MISMATCH and a reason naming both versions. Same for own-device sync.
#[tokio::test(flavor = "multi_thread")]
async fn an_old_app_dialing_us_gets_a_clear_version_mismatch() {
    for v in ALL {
        let new = endpoint(ours(v)).await;
        let addr = new.addr();
        let server = tokio::spawn(serve_one(new.clone()));
        let old = endpoint(vec![]).await;
        let conn = old.connect(addr, v.old[0]).await.unwrap();
        let why = mismatch(tokio::time::timeout(Duration::from_secs(10), conn.closed()).await.unwrap());
        assert!(why.contains(&String::from_utf8_lossy(v.old[0]).to_string()) && why.contains(&String::from_utf8_lossy(v.current).to_string()), "{why}");
        assert!(server.await.unwrap().is_none());
        old.close().await;
        new.close().await;
    }
}

// KIND: invariant (P9c, test-first)
// Us dialing an old app (it lists only `tinline/chat/1`): the dial fails with an error that
// names the old version, and the old app sees the connection closed with VERSION_MISMATCH.
#[tokio::test(flavor = "multi_thread")]
async fn dialing_an_old_app_fails_with_a_clear_error() {
    for v in ALL {
        let old = endpoint(vec![v.old[0].to_vec()]).await;
        let addr = old.addr();
        let server = tokio::spawn({
            let old = old.clone();
            async move { old.accept().await.unwrap().accept().unwrap().await.unwrap() }
        });
        let new = endpoint(vec![]).await;
        let err = connect(&new, addr, v).await.expect_err("refused");
        let text = err.to_string();
        assert!(text.contains("older app") && text.contains(&String::from_utf8_lossy(v.old[0]).to_string()), "{text}");
        let seen_by_old = server.await.unwrap();
        mismatch(tokio::time::timeout(Duration::from_secs(10), seen_by_old.closed()).await.unwrap());
        new.close().await;
        old.close().await;
    }
}

// KIND: invariant
// Two current apps: the current version is picked and the connection stays up.
#[tokio::test(flavor = "multi_thread")]
async fn two_current_apps_speak_the_current_version() {
    let a = endpoint(ours(&CHAT)).await;
    let addr = a.addr();
    let server = tokio::spawn(serve_one(a.clone()));
    let b = endpoint(vec![]).await;
    let conn = connect(&b, addr, &CHAT).await.unwrap();
    assert_eq!(conn.alpn(), CHAT.current);
    let served = server.await.unwrap().expect("kept");
    assert_eq!(served.alpn(), CHAT.current);
    assert!(conn.close_reason().is_none());
    b.close().await;
    a.close().await;
}
