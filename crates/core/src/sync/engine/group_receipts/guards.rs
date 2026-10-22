//! Real stream around the handler; registry is seeded, not paired here. The
//! two-node test separately exercises both actual self_run authentications.
use super::super::group_history::tests::{Directory, Quiet, endpoint, registry};
use super::*;
use crate::Node;

#[tokio::test(flavor = "current_thread")]
async fn unnegotiated_stale_failed_persistence_and_removed_device_do_not_confirm_proofs() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let dir = Directory::new();
        let node = Node::new(dir.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("synthetic receipt guards".into(), String::new())
            .unwrap();
        let _ = node.unlock_key().unwrap();
        node.commit_identity().unwrap();
        let me = node.inner.me().unwrap();
        let (_, target, gid, bytes) =
            crate::group::engine::receipt_cores_for_test(me.id.clone(), me.epoch, me.epoch);
        *node.inner.group.core.lock() = Some(target.clone());
        let server = endpoint(me.profile.device_secret).await;
        let client = endpoint(proto::new_device_secret()).await;
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
        let (conn, outgoing) =
            tokio::join!(accept, client.connect(server.addr(), b"tinline/self/3"));
        let outgoing = outgoing.unwrap();
        let (send, _) = conn.open_bi().await.unwrap();
        let mut writer = FrameWriter(send);
        writer.send(&SelfMsg::Hello { docs: vec![] }).await.unwrap();
        let (_, recv) = outgoing.accept_bi().await.unwrap();
        let mut reader = FrameReader::new(recv);
        reader.recv::<SelfMsg>().await.unwrap();
        let message = SelfMsg::GroupReceiptPage {
            request: 1,
            group: gid.clone(),
            after: None,
            proofs: vec![(format!("group/{gid}/1970-01-01"), bytes.clone())],
            next: Some(digest(&bytes)),
        };
        let mut peer = PeerChat::default();
        assert!(
            handle(
                &node.inner.sync,
                &me,
                &remote,
                &mut peer,
                message.clone(),
                &mut writer
            )
            .await
            .is_err()
        );
        peer.receipts.enabled = true;
        peer.receipts.waiting = Some(Waiting {
            request: 2,
            group: gid.clone(),
            after: None,
            sent: Instant::now(),
        });
        handle(
            &node.inner.sync,
            &me,
            &remote,
            &mut peer,
            message.clone(),
            &mut writer,
        )
        .await
        .unwrap();
        assert!(
            target
                .store
                .storage_attestation(&gid, &digest(&bytes))
                .unwrap()
                .is_none()
        );
        peer.receipts.waiting.as_mut().unwrap().request = 1;
        target.store.fail_next_append_for_test();
        assert!(matches!(
            handle(
                &node.inner.sync,
                &me,
                &remote,
                &mut peer,
                message.clone(),
                &mut writer
            )
            .await,
            Err(Error::Io(_))
        ));
        assert_eq!(peer.receipts.waiting.as_ref().unwrap().request, 1);
        assert!(peer.docs.is_empty());
        assert!(
            target
                .store
                .acks_day(&gid, "1970-01-01")
                .unwrap()
                .is_empty()
        );
        assert!(
            target
                .store
                .storage_attestation(&gid, &digest(&bytes))
                .unwrap()
                .is_none()
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
        assert!(
            handle(&node.inner.sync, &me, &remote, &mut peer, message, &mut writer)
                .await
                .is_err()
        );
        assert!(
            target
                .store
                .storage_attestation(&gid, &digest(&bytes))
                .unwrap()
                .is_none()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(60), reader.recv::<SelfMsg>())
                .await
                .is_err()
        );
        client.close().await;
        server.close().await;
    })
    .await
    .expect("receipt handler guards timeout");
}
