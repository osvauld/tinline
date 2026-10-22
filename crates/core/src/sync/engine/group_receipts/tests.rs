use super::super::group_history::tests::{Directory, Quiet, endpoint, registry};
use super::*;
use crate::Node;

fn marker(name: &str) -> (String, Vec<u8>) {
    (name.into(), VersionVector::default().encode())
}
#[test]
fn all_three_exact_markers_required_and_old_discriminants_stay_stable() {
    let docs = vec![
        marker(group_documents::CAPABILITY),
        marker(group_history::CAPABILITY),
        marker(CAPABILITY),
    ];
    assert!(supported(&docs));
    for omit in 0..3 {
        let mut d = docs.clone();
        d.remove(omit);
        assert!(!supported(&d));
    }
    let mut d = docs;
    d[2].1 = vec![255];
    assert!(!supported(&d));
    for (message, tag) in [
        (SelfMsg::Groups { groups: vec![] }, 6_u32),
        (
            SelfMsg::GroupHistoryChanged {
                group: "a".repeat(64),
            },
            12,
        ),
        (
            SelfMsg::GroupReceiptGet {
                request: 1,
                group: "a".repeat(64),
                after: None,
            },
            13,
        ),
    ] {
        assert_eq!(u32::from(cryptography::codec::to_bytes(&message).unwrap()[0]), tag);
    }
}
#[test]
fn refresh_is_deduplicated_and_does_not_reset_active_archive_cursor() {
    let mut r = Receipts::default();
    r.enabled = true;
    r.scopes.insert("hot".into());
    r.refresh("unknown");
    assert!(r.queue.is_empty());
    r.queue.push_back(("hot".into(), Some("c".repeat(64))));
    for _ in 0..100 {
        r.refresh("hot");
    }
    assert_eq!(r.queue.len(), 1);
    assert_eq!(r.refresh.len(), 1);
    assert_eq!(r.queue.front().unwrap().1, Some("c".repeat(64)));
}
#[test]
fn idle_rescan_deadline_is_fixed() {
    let r = Receipts {
        scan: Some(Instant::now() - RESCAN + Duration::from_secs(1)),
        ..Default::default()
    };
    assert!(r.delay() <= Duration::from_secs(1));
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_own_sessions_forward_exact_host_proof_and_reconcile_receipt_before_contents()
{
    tokio::time::timeout(Duration::from_secs(35), async {
        let ad = Directory::new(); let bd = Directory::new();
        let a = Node::new(ad.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        let phrase = a.create_identity("synthetic proof relay".into(), String::new()).unwrap();
        let _ = a.unlock_key().unwrap(); a.commit_identity().unwrap();
        let b = Node::new(bd.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        b.restore_identity(phrase, "synthetic proof phone".into(), String::new()).unwrap();
        let _ = b.unlock_key().unwrap(); b.commit_identity().unwrap();
        let ma = a.inner.me().unwrap(); let mb = b.inner.me().unwrap();
        let (source, target, gid, original) = crate::group::engine::receipt_cores_for_test(ma.id.clone(), ma.epoch, mb.epoch);
        *a.inner.group.core.lock() = Some(source.clone()); *b.inner.group.core.lock() = Some(target.clone());
        let server = endpoint(ma.profile.device_secret).await; let client = endpoint(mb.profile.device_secret).await;
        registry(&a, *client.id().as_bytes()); registry(&b, *server.id().as_bytes());
        let ra = async { let conn = server.accept().await.unwrap().accept().unwrap().await.unwrap(); a.inner.sync.clone().self_run(conn, false).await };
        let rb = async { let conn = client.connect(server.addr(), b"tinline/self/3").await.unwrap(); b.inner.sync.clone().self_run(conn, true).await };
        let monitor = async {
            loop {
                let held = target.store.storage_attestation(&gid, &digest(&original)).unwrap();
                let day = crate::group::doc::DocId::Day("1970-01-01".into());
                if held.is_some() && target.doc_vv(&gid, &day) == source.doc_vv(&gid, &day)
                    && !target.groups.lock()[&gid].meta.owed_days.contains("1970-01-01") {
                    assert_eq!(held.unwrap().bytes, original);
                    assert_eq!(target.store.acks_day(&gid, "1970-01-01").unwrap().len(), 1);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        };
        tokio::pin!(ra, rb, monitor);
        tokio::select! { result = &mut ra => panic!("relay ended early: {result:?}"), result = &mut rb => panic!("phone ended early: {result:?}"), _ = &mut monitor => {} }
        client.close().await; server.close().await;
    }).await.expect("signed receipt own-wire convergence timeout");
}
