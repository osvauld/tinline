use super::*;
use crate::chat::doc::{DAY_MS, writer_id};
use crate::group::{
    doc::GroupDoc,
    record::{Body, Record},
};
use crate::{CallInfo, CallState, Node, NodeEvents, NodeStatus};
use iroh::{Endpoint, SecretKey, endpoint::presets};

pub(in crate::sync::engine) struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}
pub(in crate::sync::engine) struct Directory(pub(in crate::sync::engine) std::path::PathBuf);
impl Directory {
    pub(in crate::sync::engine) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tinline-own-history-{}-{}",
            std::process::id(),
            hex::encode(proto::new_device_secret())
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(in crate::sync::engine) async fn endpoint(secret: [u8; 32]) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&secret))
        .clear_ip_transports()
        .clear_relay_transports()
        .clear_address_lookup()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .alpns(vec![b"tinline/self/3".to_vec()])
        .bind()
        .await
        .unwrap()
}
pub(in crate::sync::engine) fn registry(node: &Node, remote: [u8; 32]) {
    let me = node.inner.me().unwrap();
    node.inner
        .shared
        .lock()
        .state
        .registry
        .push(crate::store::OwnDevice {
            device: remote,
            attestation: proto::attest(&me.id, remote, crate::node::now()),
            label: "synthetic archive peer".into(),
            removed: false,
            last_seen: 0,
            relay: None,
        });
}
fn marker(name: &str) -> (String, Vec<u8>) {
    (name.into(), VersionVector::default().encode())
}

#[test]
fn history_needs_both_exact_empty_capability_markers() {
    assert!(!supported(&[marker(CAPABILITY)]));
    assert!(!supported(&[marker(group_documents::CAPABILITY)]));
    assert!(supported(&[
        marker(CAPABILITY),
        marker(group_documents::CAPABILITY)
    ]));
    assert!(!supported(&[
        (CAPABILITY.into(), vec![255]),
        marker(group_documents::CAPABILITY)
    ]));
}
#[test]
fn extension_keeps_existing_message_discriminants() {
    let cases = [
        (SelfMsg::Hello { docs: vec![] }, 1_u32),
        (
            SelfMsg::Sync {
                doc: String::new(),
                vv: vec![],
                update: vec![],
                sig: vec![],
            },
            2,
        ),
        (SelfMsg::Groups { groups: vec![] }, 6),
        (SelfMsg::Invitations { rows: vec![] }, 7),
        (
            SelfMsg::GroupHistoryGet {
                request: 1,
                group: "a".repeat(64),
                after: None,
            },
            8,
        ),
    ];
    for (message, tag) in cases {
        let encoded = cryptography::codec::to_bytes(&message).unwrap();
        assert_eq!(u32::from(encoded[0]), tag);
        let _: SelfMsg = cryptography::codec::from_bytes(&encoded, usize::MAX).unwrap();
    }
}

#[test]
fn frontiers_are_bounded_and_negative_counts_fail() {
    assert!(vv(&vec![0; VV_BYTES + 1]).is_err());
    assert!(vv(&[255]).is_err());
    let mut negative = VersionVector::default();
    negative.insert(1, -1);
    assert!(vv(&negative.encode()).is_err());
}
#[test]
fn change_notifications_cannot_create_a_scope_or_unbounded_queue() {
    let mut history = History::default();
    history.changed(&"a".repeat(64));
    assert!(history.queued.is_empty());
    let group = "b".repeat(64);
    history.scopes.insert(group.clone());
    for _ in 0..100 {
        history.changed(&group);
    }
    assert_eq!(history.queued.len(), 1);
}
#[test]
fn repeated_hot_scope_does_not_jump_a_waiting_scope() {
    let mut history = History::default();
    history.enqueue("hot".into());
    history.enqueue("quiet".into());
    assert_eq!(history.queued.pop_front().as_deref(), Some("hot"));
    history.queued_set.remove("hot");
    history.enqueue("hot".into());
    assert_eq!(history.queued.pop_front().as_deref(), Some("quiet"));
}

#[test]
fn idle_rescan_uses_a_fixed_deadline_not_time_since_last_other_message() {
    let mut history = History::default();
    history.last_scan = Some(Instant::now() - RESCAN + Duration::from_secs(2));
    assert!(history.delay() <= Duration::from_secs(2));
}

#[test]
fn request_ids_do_not_wrap_and_retry_keeps_original_id() {
    let mut history = History::default();
    assert_eq!(history.next_id().unwrap(), 1);
    history.serial = u64::MAX;
    assert!(history.next_id().is_err());
    let message = SelfMsg::GroupWant {
        request: 7,
        doc: "example".into(),
        vv: vec![],
    };
    assert_eq!(request_id(&message.clone()), 7);
}

#[tokio::test(flavor = "current_thread")]
async fn actual_authenticated_two_node_archive_sync_crosses_page_and_batch_boundaries() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let a_dir = Directory::new();
        let b_dir = Directory::new();
        let a = Node::new(a_dir.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        let phrase = a
            .create_identity("synthetic archive source".into(), String::new())
            .unwrap();
        let _ = a.unlock_key().unwrap();
        a.commit_identity().unwrap();
        let b = Node::new(b_dir.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        b.restore_identity(phrase, "synthetic archive receiver".into(), String::new())
            .unwrap();
        let _ = b.unlock_key().unwrap();
        b.commit_identity().unwrap();
        let ma = a.inner.me().unwrap();
        let mb = b.inner.me().unwrap();
        let (source, gid, _) =
            crate::group::engine::own_content_core_for_test(ma.id.clone(), ma.epoch, true);
        let (target, _, _) =
            crate::group::engine::own_content_core_for_test(mb.id.clone(), mb.epoch, false);
        *a.inner.group.core.lock() = Some(source.clone());
        *b.inner.group.core.lock() = Some(target.clone());
        // 70 cold days plus the fixture's recent day. One cold day has more
        // than the 64-operation wire budget; originals are not re-authored.
        let meta_seen = source
            .doc_vv(&gid, &DocId::Meta)
            .iter()
            .map(|(p, c)| (*p, *c))
            .collect::<Vec<_>>();
        let mut meta_seen = meta_seen;
        meta_seen.sort();
        for i in 1..=70 {
            let at = i * DAY_MS;
            let day = crate::chat::doc::day_of(at);
            let dev = [1; 32];
            let doc = GroupDoc::new_in(&gid, DocId::Day(day.clone()), writer_id(ma.id.did(), &dev));
            for j in 0..if i == 1 { 70 } else { 1 } {
                doc.write(
                    &ma.id,
                    &Record {
                        group: gid.clone(),
                        author: ma.id.did().into(),
                        dev,
                        at,
                        seen: meta_seen.clone(),
                        body: Body::Post {
                            text: format!("cold original {i}/{j}"),
                            reply_to: None,
                        },
                    },
                )
                .unwrap();
            }
            source
                .own_content_apply(
                    &gid,
                    &DocId::Day(day),
                    &doc.export_since(&VersionVector::default()).unwrap(),
                )
                .unwrap();
        }
        let server = endpoint(ma.profile.device_secret).await;
        let client = endpoint(mb.profile.device_secret).await;
        registry(&a, *client.id().as_bytes());
        registry(&b, *server.id().as_bytes());
        let run_a = async {
            let incoming = server
                .accept()
                .await
                .unwrap()
                .accept()
                .unwrap()
                .await
                .unwrap();
            a.inner.clone().self_run(incoming, false).await
        };
        let run_b = async {
            let outgoing = client
                .connect(server.addr(), b"tinline/self/3")
                .await
                .unwrap();
            b.inner.clone().self_run(outgoing, true).await
        };
        let monitor = async {
            loop {
                let complete = (1..=70).all(|i| {
                    let doc = DocId::Day(crate::chat::doc::day_of(i * DAY_MS));
                    target.doc_vv(&gid, &doc) == source.doc_vv(&gid, &doc)
                });
                if complete {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let doc = DocId::Day(crate::chat::doc::day_of(DAY_MS));
            let (original, _) = source
                .own_content_export(&gid, &doc, &VersionVector::default())
                .unwrap()
                .unwrap();
            let (copied, _) = target
                .own_content_export(&gid, &doc, &VersionVector::default())
                .unwrap()
                .unwrap();
            let mut left = GroupDoc::new_in(&gid, doc.clone(), 77);
            let mut right = GroupDoc::new_in(&gid, doc, 88);
            left.apply_remote(&original, |_| Ok(())).unwrap();
            right.apply_remote(&copied, |_| Ok(())).unwrap();
            assert_eq!(left.entries().unwrap(), right.entries().unwrap());
            for entry in left.entries().unwrap() {
                assert_eq!(left.raw_value(&entry.id), right.raw_value(&entry.id));
            }
        };
        tokio::pin!(run_a, run_b, monitor);
        tokio::select! {
            result = &mut run_a => panic!("source session ended before catch-up: {result:?}"),
            result = &mut run_b => panic!("receiver session ended before catch-up: {result:?}"),
            _ = &mut monitor => {}
        }
        client.close().await;
        server.close().await;
    })
    .await
    .expect("bounded archive sync timeout");
}
