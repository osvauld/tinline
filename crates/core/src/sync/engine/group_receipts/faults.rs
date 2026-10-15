//! Actual authenticated receiver self_run with an application-message fault peer.
//! No encrypted QUIC packet loss or process-kill is simulated.
use super::super::group_history::tests::{Directory, Quiet, endpoint, registry};
use super::*;
use crate::{
    Node,
    group::{doc::DocId, engine::GroupCore},
};
use iroh::Endpoint;

async fn session(
    node: &Node,
    server: &Endpoint,
    client: &Endpoint,
    secret: &[u8; 32],
    source: &Arc<GroupCore>,
    gid: &str,
    original: &[u8],
    faults: bool,
    legacy: bool,
) -> usize {
    let me = node.inner.me().unwrap();
    let receiver = node.inner.group_core().unwrap();
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
        let mut docs = vec![
            (
                group_documents::CAPABILITY.into(),
                VersionVector::default().encode(),
            ),
            (
                group_history::CAPABILITY.into(),
                VersionVector::default().encode(),
            ),
        ];
        if !legacy {
            docs.push((CAPABILITY.into(), VersionVector::default().encode()));
        }
        writer.send(&SelfMsg::Hello { docs }).await.unwrap();
        let token = InviteToken {
            group: gid.into(),
            name: "synthetic receipt faults".into(),
            owner: me.id.did().into(),
            hosts: vec![],
            invite: None,
        };
        writer
            .send(&SelfMsg::Groups {
                groups: vec![cryptography::codec::to_bytes(&token).unwrap()],
            })
            .await
            .unwrap();
        let mut requests = 0;
        let mut first_id = None;
        let mut scanned = false;
        loop {
            let message = if legacy && scanned {
                match tokio::time::timeout(Duration::from_millis(200), reader.recv::<SelfMsg>())
                    .await
                {
                    Err(_) => {
                        conn.close(0u32.into(), b"legacy checked");
                        return requests;
                    }
                    Ok(result) => result.unwrap().unwrap(),
                }
            } else {
                reader.recv::<SelfMsg>().await.unwrap().unwrap()
            };
            match message {
                SelfMsg::GroupWant { request, doc, vv } => {
                    let (group, which) = group_documents::parse_doc(&doc).unwrap();
                    assert_eq!(group, gid);
                    let their = VersionVector::decode(&vv).unwrap();
                    let out = source
                        .own_content_export_batch(
                            gid,
                            &which,
                            &their,
                            crate::group::doc::batching::BatchLimits {
                                bytes: 256 * 1024,
                                operations: 64,
                                writers: 4096,
                            },
                        )
                        .unwrap();
                    let (update, vv, more) = out
                        .map(|o| (o.update, o.frontier.encode(), o.more))
                        .unwrap_or((vec![], VersionVector::default().encode(), false));
                    let sig = crate::chat::wire::sign_batch(secret, &doc, &update);
                    writer
                        .send(&SelfMsg::GroupBatch {
                            request,
                            doc,
                            vv,
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
                    writer
                        .send(&SelfMsg::GroupHistoryPage {
                            request,
                            group,
                            after,
                            days: vec![],
                            next: None,
                            deferred: false,
                        })
                        .await
                        .unwrap();
                    scanned = true;
                }
                SelfMsg::GroupReceiptGet {
                    request,
                    group,
                    after,
                } => {
                    assert!(!legacy, "legacy peer received receipt frame");
                    assert_eq!(group, gid);
                    requests += 1;
                    if after.is_some() {
                        assert_eq!(after, Some(digest(original)));
                        writer
                            .send(&SelfMsg::GroupReceiptPage {
                                request,
                                group,
                                after,
                                proofs: vec![],
                                next: None,
                            })
                            .await
                            .unwrap();
                        assert_eq!(
                            receiver
                                .store
                                .storage_attestation(gid, &digest(original))
                                .unwrap()
                                .unwrap()
                                .bytes,
                            original
                        );
                        conn.close(0u32.into(), b"receipt scan checked");
                        return requests;
                    }
                    if faults {
                        if requests == 1 {
                            first_id = Some(request);
                            continue;
                        } // lost WANT
                        assert_eq!(Some(request), first_id, "retry changed request ID");
                    }
                    let proofs = source
                        .storage_attestation_page(gid, after.as_deref())
                        .unwrap();
                    let page = SelfMsg::GroupReceiptPage {
                        request,
                        group: group.clone(),
                        after: after.clone(),
                        next: proofs.last().map(|(_, b)| digest(b)),
                        proofs,
                    };
                    if faults && requests == 2 {
                        continue;
                    } // computed reply lost
                    if faults {
                        let obsolete = SelfMsg::GroupReceiptPage {
                            request: request.checked_add(1).unwrap(),
                            group,
                            after,
                            proofs: vec![(format!("group/{gid}/1970-01-01"), original.to_vec())],
                            next: Some(digest(original)),
                        };
                        writer.send(&obsolete).await.unwrap();
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        assert!(
                            receiver
                                .store
                                .storage_attestation(gid, &digest(original))
                                .unwrap()
                                .is_none()
                        );
                    }
                    writer.send(&page).await.unwrap();
                    writer.send(&page).await.unwrap(); // duplicate, not a second confirmation
                    if faults {
                        loop {
                            if receiver
                                .store
                                .storage_attestation(gid, &digest(original))
                                .unwrap()
                                .is_some()
                            {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        // Interrupt after durable acceptance, before inventory continuation.
                        conn.close(0u32.into(), b"receipt interrupted after commit");
                        return requests;
                    }
                }
                _ => {} // cache Sync and unrelated own-account traffic are not host proof
            }
        }
    };
    tokio::pin!(receiving, driving);
    tokio::select! { result = &mut receiving => panic!("receiver ended before faults finished: {result:?}"), count = &mut driving => count }
}

#[tokio::test(flavor = "current_thread")]
async fn lost_request_reply_duplicate_and_reconnect_keep_original_proof_and_legacy_is_silent() {
    tokio::time::timeout(Duration::from_secs(40), async {
        let dir = Directory::new();
        let node = Node::new(dir.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("synthetic receipt fault receiver".into(), String::new())
            .unwrap();
        let _ = node.unlock_key().unwrap();
        node.commit_identity().unwrap();
        let me = node.inner.me().unwrap();
        let (source, target, gid, original) =
            crate::group::engine::receipt_cores_for_test(me.id.clone(), me.epoch, me.epoch);
        let day = DocId::Day("1970-01-01".into());
        let (update, _) = source
            .own_content_export(&gid, &day, &VersionVector::default())
            .unwrap()
            .unwrap();
        target.own_content_apply(&gid, &day, &update).unwrap();
        assert!(
            target.groups.lock()[&gid]
                .meta
                .owed_days
                .contains("1970-01-01")
        );
        *node.inner.group.core.lock() = Some(target.clone());
        let secret = proto::new_device_secret();
        let server = endpoint(me.profile.device_secret).await;
        let client = endpoint(secret).await;
        registry(&node, *client.id().as_bytes());
        assert_eq!(
            session(
                &node, &server, &client, &secret, &source, &gid, &original, true, false
            )
            .await,
            3
        );
        assert!(
            !target.groups.lock()[&gid]
                .meta
                .owed_days
                .contains("1970-01-01")
        );
        assert_eq!(
            session(
                &node, &server, &client, &secret, &source, &gid, &original, false, false
            )
            .await,
            2
        );
        assert_eq!(
            target
                .store
                .storage_attestation_page(&gid, None, 2)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(target.store.acks_day(&gid, "1970-01-01").unwrap().len(), 1);
        assert_eq!(
            session(
                &node, &server, &client, &secret, &source, &gid, &original, false, true
            )
            .await,
            0
        );
        client.close().await;
        server.close().await;
    })
    .await
    .expect("receipt fault/reconnect timeout");
}
