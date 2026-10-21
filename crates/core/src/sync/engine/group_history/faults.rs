//! Fault-injecting authenticated peer around the actual receiver self_run.
//! These drop application messages/receipt processing, not encrypted QUIC packets.
use super::{
    tests::{Directory, Quiet, endpoint, registry},
    *,
};
use crate::{
    Node,
    group::{doc::GroupDoc, engine::GroupCore},
};
use iroh::Endpoint;

#[derive(Default, Debug)]
struct Observed {
    page_requests: usize,
    day_requests: usize,
    dropped_page: bool,
    dropped_want: bool,
    suppressed_cache_ack: bool,
    stale_sent: bool,
    meta_want: Option<VersionVector>,
}

fn durable_day_owed(core: &GroupCore, gid: &str, day: &str) -> bool {
    core.store
        .groups()
        .unwrap()
        .into_iter()
        .find(|(g, _)| g == gid)
        .unwrap()
        .1
        .owed_days
        .contains(day)
}

async fn session(
    node: &Node,
    server: &Endpoint,
    client: &Endpoint,
    secret: &[u8; 32],
    source: &Arc<GroupCore>,
    gid: &str,
    day: &str,
    interrupt: bool,
    replay: Option<SelfMsg>,
) -> Observed {
    let me = node.inner.me().unwrap();
    let name = format!("group/{gid}/{day}");
    let token = cryptography::codec::to_bytes(&InviteToken {
        group: gid.into(),
        name: "synthetic fault scope".into(),
        owner: me.id.did().into(),
        hosts: vec![],
        invite: None,
        open: false,
    })
    .unwrap();
    let receiving = async {
        let incoming = server
            .accept()
            .await
            .unwrap()
            .accept()
            .unwrap()
            .await
            .unwrap();
        node.inner.clone().self_run(incoming, false).await
    };
    let driving = async {
        let conn = client
            .connect(server.addr(), b"tinline/self/3")
            .await
            .unwrap();
        let (send, recv) = conn.open_bi().await.unwrap();
        let mut writer = FrameWriter(send);
        let mut reader = FrameReader::new(recv);
        writer
            .send(&SelfMsg::Auth {
                attestation: proto::attest(&me.id, *client.id().as_bytes(), crate::node::now()),
                relay: None,
                unlink: false,
            })
            .await
            .unwrap();
        assert!(matches!(
            reader.recv::<SelfMsg>().await.unwrap(),
            Some(SelfMsg::Auth { .. })
        ));
        assert!(matches!(
            reader.recv::<SelfMsg>().await.unwrap(),
            Some(SelfMsg::Hello { .. })
        ));
        writer
            .send(&SelfMsg::Hello {
                docs: vec![
                    (
                        group_documents::CAPABILITY.into(),
                        VersionVector::default().encode(),
                    ),
                    (CAPABILITY.into(), VersionVector::default().encode()),
                ],
            })
            .await
            .unwrap();
        writer
            .send(&SelfMsg::Groups {
                groups: vec![token],
            })
            .await
            .unwrap();
        let mut observed = Observed::default();
        let mut dropped_want_id = None;
        let mut dropped_page_id = None;
        let mut replay = replay;
        loop {
            let message = reader
                .recv::<SelfMsg>()
                .await
                .unwrap()
                .expect("receiver ended before fault recovery");
            match message {
                SelfMsg::GroupWant {
                    request,
                    doc,
                    vv: bytes,
                } => {
                    let (_, which) = group_documents::parse_doc(&doc).unwrap();
                    let theirs = vv(&bytes).unwrap();
                    if which == DocId::Meta {
                        observed.meta_want = Some(theirs.clone());
                        if let Some(old) = replay.take() {
                            // Old day response while the fresh session awaits metadata.
                            writer.send(&old).await.unwrap();
                            observed.stale_sent = true;
                        }
                    }
                    if doc == name {
                        observed.day_requests += 1;
                        if interrupt && !observed.dropped_want {
                            dropped_want_id = Some(request);
                            observed.dropped_want = true;
                            continue; // request never reaches the source planner
                        }
                        if let Some(old_id) = dropped_want_id {
                            assert_eq!(request, old_id, "retry changed outstanding request ID");
                        }
                    }
                    let out = source
                        .own_content_export_batch(
                            gid,
                            &which,
                            &theirs,
                            BatchLimits {
                                bytes: BATCH_BYTES,
                                operations: 64,
                                writers: 4096,
                            },
                        )
                        .unwrap();
                    let (update, frontier, more) = out
                        .map(|b| (b.update, b.frontier.encode(), b.more))
                        .unwrap_or((vec![], VersionVector::default().encode(), false));
                    let sig = crate::chat::wire::sign_batch(secret, &doc, &update);
                    writer
                        .send(&SelfMsg::GroupBatch {
                            request,
                            doc,
                            vv: frontier,
                            update,
                            sig,
                            more,
                            deferred: false,
                        })
                        .await
                        .unwrap();
                }
                SelfMsg::GroupHistoryGet {
                    request,
                    group,
                    after,
                } => {
                    assert_eq!(group, gid);
                    observed.page_requests += 1;
                    let page = source
                        .own_history_page(gid, after.as_deref(), PAGE_DAYS)
                        .unwrap();
                    if interrupt && !observed.dropped_page {
                        observed.dropped_page = true;
                        dropped_page_id = Some(request);
                        continue; // response computed but lost before delivery
                    }
                    if after.is_none()
                        && let Some(old_id) = dropped_page_id
                    {
                        assert_eq!(request, old_id, "page retry changed outstanding request ID");
                    }
                    if !interrupt && after.as_deref() == Some(day) {
                        // Fresh scan reached EOF without requesting the already persisted day.
                        assert!(page.days.is_empty());
                        conn.close(0u32.into(), b"fault test complete");
                        return observed;
                    }
                    writer
                        .send(&SelfMsg::GroupHistoryPage {
                            request,
                            group,
                            after,
                            days: page.days,
                            next: page.next,
                            deferred: false,
                        })
                        .await
                        .unwrap();
                }
                SelfMsg::Sync {
                    doc,
                    vv: bytes,
                    update,
                    sig,
                } if doc == name => {
                    assert!(update.is_empty());
                    assert!(chat_sig_ok(&me.device, &doc, &[], &sig));
                    assert_eq!(
                        vv(&bytes).unwrap(),
                        source.doc_vv(gid, &DocId::Day(day.into()))
                    );
                    assert!(
                        interrupt,
                        "stale response unexpectedly generated a new day receipt"
                    );
                    // Suppress processing of this local-cache Ack, then lose the link.
                    // There is intentionally no source peer-VV advancement here.
                    observed.suppressed_cache_ack = true;
                    conn.close(
                        0u32.into(),
                        b"after durable import before source Ack processing",
                    );
                    return observed;
                }
                // Tokens, change hints and metadata cache receipts are orthogonal.
                SelfMsg::Groups { .. }
                | SelfMsg::GroupHistoryChanged { .. }
                | SelfMsg::Sync { .. }
                | SelfMsg::Read { .. }
                | SelfMsg::Acks { .. }
                | SelfMsg::Invitations { .. } => {}
                other => panic!("unexpected fault-driver message: {other:?}"),
            }
        }
    };
    let (_closed_session, observed) = tokio::join!(receiving, driving);
    observed
}

#[tokio::test(flavor = "current_thread")]
async fn lost_want_page_and_cache_ack_reconnect_from_durable_frontier_without_duplicate_or_host_ack()
 {
    tokio::time::timeout(Duration::from_secs(50), async {
        let directory = Directory::new();
        let node = Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("synthetic fault receiver".into(), String::new())
            .unwrap();
        let _ = node.unlock_key().unwrap();
        node.commit_identity().unwrap();
        let me = node.inner.me().unwrap();
        let (source, gid, day) =
            crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, true);
        let (target, _, _) =
            crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, false);
        *node.inner.group.core.lock() = Some(target.clone());
        let server = endpoint(me.profile.device_secret).await;
        let secret = proto::new_device_secret();
        let client = endpoint(secret).await;
        registry(&node, *client.id().as_bytes());
        let first = session(
            &node, &server, &client, &secret, &source, &gid, &day, true, None,
        )
        .await;
        assert!(first.dropped_want && first.dropped_page && first.suppressed_cache_ack);
        assert_eq!(first.day_requests, 2);
        assert!(first.page_requests >= 2);
        let which = DocId::Day(day.clone());
        let (snapshot, updates) = target.store.shard_rows(&gid, &day).unwrap();
        let accepted = GroupDoc::load_in(
            &gid,
            which.clone(),
            target.wid,
            snapshot.as_deref(),
            &updates,
        )
        .unwrap();
        assert_eq!(accepted.entries().unwrap().len(), 1);
        assert_eq!(accepted.vv(), source.doc_vv(&gid, &which));
        // A cache Ack must not become an external host-storage Ack or clear owed data.
        assert!(target.store.acks_day(&gid, &day).unwrap().is_empty());
        assert!(target.store.acks(&gid).unwrap().is_empty());
        assert!(durable_day_owed(&target, &gid, &day));

        // Construct a valid original operation that the source has NOT persisted.
        // An obsolete-response replay must not sneak it into the fresh session.
        let original = accepted.entries().unwrap().remove(0).rec;
        let author_peer = crate::chat::doc::writer_id(&original.author, &original.dev);
        let stale_source = GroupDoc::load_in(
            &gid,
            which.clone(),
            author_peer,
            snapshot.as_deref(),
            &updates,
        )
        .unwrap();
        let mut later = original.clone();
        later.body = crate::group::record::Body::Post {
            text: "obsolete callback must not import this".into(),
            reply_to: None,
        };
        let (_, stale_update) = stale_source.write(&me.id, &later).unwrap();
        let doc = format!("group/{gid}/{day}");
        let stale = SelfMsg::GroupBatch {
            request: 3,
            doc: doc.clone(),
            vv: stale_source.vv().encode(),
            sig: crate::chat::wire::sign_batch(&secret, &doc, &stale_update),
            update: stale_update,
            more: false,
            deferred: false,
        };
        let second = session(
            &node,
            &server,
            &client,
            &secret,
            &source,
            &gid,
            &day,
            false,
            Some(stale),
        )
        .await;
        assert!(second.stale_sent);
        assert_eq!(
            second.day_requests, 0,
            "lost Ack caused unnecessary day replay instead of actual-frontier reconciliation"
        );
        assert!(covers(
            &second.meta_want.unwrap(),
            &source.doc_vv(&gid, &DocId::Meta)
        ));
        let (snap, rows) = target.store.shard_rows(&gid, &day).unwrap();
        let reopened_view =
            GroupDoc::load_in(&gid, which, target.wid, snap.as_deref(), &rows).unwrap();
        assert_eq!(reopened_view.vv(), accepted.vv());
        assert_eq!(
            reopened_view.entries().unwrap(),
            accepted.entries().unwrap()
        );
        for entry in accepted.entries().unwrap() {
            assert_eq!(
                reopened_view.raw_value(&entry.id),
                accepted.raw_value(&entry.id)
            );
        }
        assert!(target.store.acks_day(&gid, &day).unwrap().is_empty());
        assert!(durable_day_owed(&target, &gid, &day));
        client.close().await;
        server.close().await;
    })
    .await
    .expect("fault recovery session timeout");
}
