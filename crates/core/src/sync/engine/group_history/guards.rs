//! Handler guards on a real localhost stream with synthetic authenticated-registry
//! fixtures. The separate two-node test exercises the actual self_run handshake.
use super::{
    tests::{Directory, Quiet, endpoint, registry},
    *,
};
use crate::{Node, group::record::InviteToken};

#[tokio::test(flavor = "current_thread")]
async fn stale_malformed_unnegotiated_revoked_and_failed_persistence_emit_no_ack() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let directory = Directory::new();
        let node = Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("synthetic history guards".into(), String::new())
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
        let client_secret = proto::new_device_secret();
        let client = endpoint(client_secret).await;
        let remote = *client.id().as_bytes();
        registry(&node, remote);
        let accept = async {
            server
                .accept()
                .await
                .unwrap()
                .accept()
                .unwrap()
                .await
                .unwrap()
        };
        let (connection, other) =
            tokio::join!(accept, client.connect(server.addr(), b"tinline/self/3"));
        let other = other.unwrap();
        let (send, _) = connection.open_bi().await.unwrap();
        let mut writer = FrameWriter(send);
        // Materialize the stream so absence of subsequent receipts can be observed.
        writer.send(&SelfMsg::Hello { docs: vec![] }).await.unwrap();
        let (_, recv) = other.accept_bi().await.unwrap();
        let mut reader = FrameReader::new(recv);
        assert!(matches!(
            reader.recv::<SelfMsg>().await.unwrap(),
            Some(SelfMsg::Hello { .. })
        ));
        let token = cryptography::codec::to_bytes(&InviteToken {
            group: gid.clone(),
            name: "hint".into(),
            owner: me.id.did().into(),
            hosts: vec![],
            invite: None,
            open: false,
        })
        .unwrap();
        node.inner.group.group_own_apply(&[token], true);
        let meta_name = format!("group/{gid}/meta");
        let (meta, _) = source
            .own_content_export(&gid, &DocId::Meta, &VersionVector::default())
            .unwrap()
            .unwrap();
        node.inner.group
            .group_own_content_apply(me.epoch, &remote, &meta_name, &meta)
            .unwrap();
        let doc = format!("group/{gid}/{day}");
        let want = SelfMsg::GroupWant {
            request: 1,
            doc: doc.clone(),
            vv: VersionVector::default().encode(),
        };
        let mut peer = PeerChat::default();
        assert!(
            handle(
                &node.inner,
                &me,
                &remote,
                &mut peer,
                want.clone(),
                &mut writer
            )
            .await
            .is_err()
        );
        peer.history.enabled = true;
        assert!(
            handle(
                &node.inner,
                &me,
                &remote,
                &mut peer,
                SelfMsg::GroupHistoryGet {
                    request: 1,
                    group: gid.clone(),
                    after: Some("not-a-day".into())
                },
                &mut writer
            )
            .await
            .is_err()
        );
        peer.history.current = Some(Scope {
            gid: gid.clone(),
            meta_done: true,
            cursor: None,
            eof: false,
            page_ready: false,
        });
        peer.history.awaiting = Some(Awaiting {
            message: want.clone(),
            sent: Instant::now(),
        });
        let (update, planned) = source
            .own_content_export(&gid, &DocId::Day(day.clone()), &VersionVector::default())
            .unwrap()
            .unwrap();
        let sig = crate::chat::wire::sign_batch(&client_secret, &doc, &update);
        let batch = SelfMsg::GroupBatch {
            request: 1,
            doc: doc.clone(),
            vv: planned.clone(),
            update: update.clone(),
            sig: sig.clone(),
            more: false,
            deferred: false,
        };
        let stale = SelfMsg::GroupBatch {
            request: 999,
            doc: doc.clone(),
            vv: planned.clone(),
            update: update.clone(),
            sig: sig.clone(),
            more: false,
            deferred: false,
        };
        handle(&node.inner, &me, &remote, &mut peer, stale, &mut writer)
            .await
            .unwrap();
        assert_eq!(
            target.doc_vv(&gid, &DocId::Day(day.clone())),
            VersionVector::default()
        );
        assert!(peer.history.awaiting.is_some());
        let forged = SelfMsg::GroupBatch {
            request: 1,
            doc: doc.clone(),
            vv: planned.clone(),
            update: update.clone(),
            sig: vec![0; 64],
            more: false,
            deferred: false,
        };
        assert!(
            handle(&node.inner, &me, &remote, &mut peer, forged, &mut writer)
                .await
                .is_err()
        );
        target.store.fail_next_append_for_test();
        assert!(matches!(
            handle(
                &node.inner,
                &me,
                &remote,
                &mut peer,
                batch.clone(),
                &mut writer
            )
            .await,
            Err(Error::Io(_))
        ));
        assert_eq!(
            target.doc_vv(&gid, &DocId::Day(day.clone())),
            VersionVector::default()
        );
        node.inner
            .shared
            .lock()
            .state
            .registry
            .iter_mut()
            .find(|d| d.device == remote)
            .unwrap()
            .removed = true;
        assert!(matches!(
            handle(&node.inner, &me, &remote, &mut peer, want, &mut writer).await,
            Err(Error::Rejected(_))
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(150), reader.recv::<SelfMsg>())
                .await
                .is_err(),
            "rejected/stale import sent an Ack"
        );
        client.close().await;
        server.close().await;
    })
    .await
    .unwrap();
}
