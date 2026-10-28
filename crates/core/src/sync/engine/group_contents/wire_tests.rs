//! Localhost self-sync tests, never QR pairing or Android integration tests.
//! Producer tests seed registry/PeerChat; the reader-failure test additionally
//! runs the real self_run DID/device auth handshake with synthetic identities.
use super::*;
use crate::chat::doc::writer_id;
use group_runtime::doc::{DocId, GroupDoc};
use crate::group::record::InviteToken;
use crate::{CallInfo, CallState, Node, NodeEvents, NodeStatus};
use iroh::{Endpoint, SecretKey, endpoint::presets};

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tinline-owned-content-wire-{}-{}",
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
async fn endpoint(secret: SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(secret)
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
fn registry(node: &Node, remote: [u8; 32]) {
    let me = node.open().me().unwrap();
    let attestation = proto::attest(&me.id, remote, crate::node::now());
    assert_eq!(
        proto::verify_attestation(&attestation).unwrap().did,
        me.id.did()
    );
    node.open()
        .shared
        .lock()
        .state
        .registry
        .push(crate::store::OwnDevice {
            device: remote,
            attestation,
            label: "synthetic wire peer".into(),
            removed: false,
            last_seen: 0,
            relay: None,
            links: Vec::new(),
            rotated: false,
        });
}

#[tokio::test(flavor = "current_thread")]
async fn actual_authenticated_self_reader_does_not_ack_failed_group_append() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let directory = Directory::new();
        let node = Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("synthetic self reader".into(), String::new())
            .unwrap();
        // Commit only this test's synthetic account files, not Git or user data.
        let _test_only_unlock_key = node.unlock_key().unwrap();
        node.commit_identity().unwrap();
        let me = node.open().me().unwrap();
        let (source, gid, day) =
            crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, true);
        let (target, _, _) =
            crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, false);
        *node.open().group.core.lock() = Some(target.clone());
        let server = endpoint(SecretKey::from_bytes(&me.profile.device_secret)).await;
        let client_secret = proto::new_device_secret();
        let client = endpoint(SecretKey::from_bytes(&client_secret)).await;
        let remote = *client.id().as_bytes();
        registry(&node, remote);
        let token = cryptography::codec::to_bytes(&InviteToken {
            group: gid.clone(),
            name: "hint only".into(),
            owner: me.id.did().into(),
            hosts: Vec::new(),
            invite: None,
            open: false,
        })
        .unwrap();
        node.open().group.group_own_apply(&[token], true);
        let (meta, _) = source
            .own_content_export(&gid, &DocId::Meta, &VersionVector::default())
            .unwrap()
            .unwrap();
        node.open().group
            .group_own_content_apply(
                me.epoch,
                &remote,
                &contents::doc_name(&gid, &DocId::Meta),
                &meta,
            )
            .unwrap();
        let which = DocId::Day(day.clone());
        let name = contents::doc_name(&gid, &which);
        let (update, vv) = source
            .own_content_export(&gid, &which, &VersionVector::default())
            .unwrap()
            .unwrap();
        target.store.fail_next_append_for_test();
        let run = async {
            let accepting = server.accept().await.unwrap().accept().unwrap();
            node.open().sync.clone().handle_self_incoming(accepting).await
        };
        let drive = async {
            let conn = client
                .connect(server.addr(), b"tinline/self/3")
                .await
                .unwrap();
            let (send, recv) = conn.open_bi().await.unwrap();
            let mut writer = FrameWriter(send);
            let mut reader = FrameReader::new(recv);
            writer
                .send(&SelfMsg::Auth {
                    attestation: proto::attest(&me.id, remote, crate::node::now()),
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
                            contents::CAPABILITY.into(),
                            VersionVector::default().encode(),
                        ),
                        (name.clone(), vv.clone()),
                    ],
                })
                .await
                .unwrap();
            let sig = chat_runtime::chat::wire::sign_batch(&client_secret, &name, &update);
            writer
                .send(&SelfMsg::Sync {
                    doc: name.clone(),
                    vv,
                    update,
                    sig,
                })
                .await
                .unwrap();
            let mut acks = 0;
            loop {
                match reader.recv::<SelfMsg>().await {
                    Ok(Some(SelfMsg::Sync { doc, update, .. }))
                        if doc == name && update.is_empty() =>
                    {
                        acks += 1
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break,
                }
            }
            assert_eq!(acks, 0);
        };
        let (result, ()) = tokio::join!(run, drive);
        assert!(matches!(result, Err(Error::Io(_))));
        assert_eq!(target.doc_vv(&gid, &which), VersionVector::default());
        assert!(target.store.shard_meta(&gid, &day).unwrap().is_none());
        assert!(!target.groups.lock()[&gid].meta.owed_days.contains(&day));
        client.close().await;
        server.close().await;
    })
    .await
    .unwrap();
}

async fn produced(negotiated: bool) {
    let directory = Directory::new();
    let node = Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.create_identity("synthetic producer".into(), String::new())
        .unwrap();
    let me = node.open().me().unwrap();
    let (core, gid, day) =
        crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, true);
    *node.open().group.core.lock() = Some(core.clone());
    let server = endpoint(SecretKey::from_bytes(&me.profile.device_secret)).await;
    let client = endpoint(SecretKey::generate()).await;
    let remote = *client.id().as_bytes();
    registry(&node, remote);
    let (outgoing, incoming) =
        tokio::join!(client.connect(server.addr(), b"tinline/self/3"), async {
            server.accept().await.unwrap().await
        });
    let (client_conn, server_conn) = (outgoing.unwrap(), incoming.unwrap());
    let (produced, consumed) = tokio::join!(
        async {
            let (send, _) = server_conn.open_bi().await.unwrap();
            let mut writer = FrameWriter(send);
            let mut peer = PeerChat {
                hello: true,
                group_contents: negotiated,
                ..Default::default()
            };
            node.open().sync
                .push_all_chat(&me, &remote, &mut peer, &mut writer)
                .await
                .unwrap();
            assert!(peer.acks.is_empty()); // Cache knowledge never becomes contact delivery.
            writer.finish();
        },
        async {
            let (_, recv) = client_conn.accept_bi().await.unwrap();
            let mut reader = FrameReader::new(recv);
            let mut messages = Vec::new();
            while let Some(frame) = reader.recv::<SelfMsg>().await.unwrap() {
                messages.push(frame);
            }
            messages
        }
    );
    let _ = produced;
    let mut docs = Vec::new();
    let mut checked_meta = GroupDoc::new(&gid, writer_id(me.id.did(), &[8; 32]));
    let mut checked_day = GroupDoc::new_in(
        &gid,
        DocId::Day(day.clone()),
        writer_id(me.id.did(), &[8; 32]),
    );
    let mut saw_tokens = false;
    for message in consumed {
        match message {
            SelfMsg::Groups { groups } => {
                assert!(
                    groups
                        .iter()
                        .any(|g| cryptography::codec::from_bytes::<InviteToken>(g, usize::MAX).unwrap().group == gid)
                );
                saw_tokens = true;
            }
            SelfMsg::Sync {
                doc,
                vv,
                update,
                sig,
            } if contents::parse_doc(&doc).is_some() => {
                assert!(negotiated && saw_tokens);
                assert!(chat_sig_ok(server.id().as_bytes(), &doc, &update, &sig));
                let (_, which) = contents::parse_doc(&doc).unwrap();
                let checked = if which == DocId::Meta {
                    &mut checked_meta
                } else {
                    &mut checked_day
                };
                checked.apply_remote(&update, |_| Ok(())).unwrap();
                assert_eq!(checked.vv(), VersionVector::decode(&vv).unwrap());
                docs.push(which);
            }
            _ => {}
        }
    }
    assert!(saw_tokens);
    if negotiated {
        assert_eq!(docs, vec![DocId::Meta, DocId::Day(day.clone())]);
        let entries = checked_day.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rec.author, me.id.did());
        assert_eq!(entries[0].rec.dev, [1; 32]); // Original phone, NOT laptop group dev [4].
        assert_eq!(
            checked_day.vv().get(&writer_id(me.id.did(), &[1; 32])),
            Some(&1)
        );
        assert!(checked_day.vv().get(&core.wid).is_none());
    } else {
        assert!(docs.is_empty());
    }
    client_conn.close(0u32.into(), b"owned test done");
    server_conn.close(0u32.into(), b"owned test done");
    client.close().await;
    server.close().await;
}

#[tokio::test(flavor = "current_thread")]
async fn negotiated_sender_emits_tokens_then_meta_then_day_with_original_phone_writer() {
    tokio::time::timeout(Duration::from_secs(20), produced(true))
        .await
        .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn legacy_sender_emits_group_tokens_but_no_group_contents() {
    tokio::time::timeout(Duration::from_secs(20), produced(false))
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn no_host_hint_bootstrap_grants_no_role_until_signed_meta_and_borrowed_writer_requires_full_ack()
 {
    tokio::time::timeout(Duration::from_secs(20), async {
        let directory = Directory::new();
        let node = Node::new(directory.0.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
        node.create_identity("synthetic receiving replica".into(), String::new())
            .unwrap();
        let me = node.open().me().unwrap();
        let (source, gid, day) =
            crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, true);
        let (target, target_gid, _) =
            crate::group::engine::own_content_core_for_test(me.id.clone(), me.epoch, false);
        assert_eq!(gid, target_gid);
        *node.open().group.core.lock() = Some(target.clone());
        let remote = cryptography::signature::public_key(&proto::new_device_secret());
        registry(&node, remote);
        let token = cryptography::codec::to_bytes(&InviteToken {
            group: gid.clone(),
            name: "hint only".into(),
            owner: me.id.did().into(),
            hosts: Vec::new(),
            invite: None,
            open: false,
        })
        .unwrap();
        node.open().group.group_own_apply(std::slice::from_ref(&token), false);
        assert!(!target.groups.lock().contains_key(&gid));
        node.open().group.group_own_apply(&[token], true);
        assert!(target.groups.lock().contains_key(&gid));
        assert!(!target.groups.lock()[&gid].state.is_member(me.id.did()));
        assert!(
            !target.groups.lock()[&gid]
                .state
                .is_host(me.id.did(), &target.dev)
        );
        let (meta, _) = source
            .own_content_export(&gid, &DocId::Meta, &VersionVector::default())
            .unwrap()
            .unwrap();
        node.open().group
            .group_own_content_apply(
                me.epoch,
                &remote,
                &contents::doc_name(&gid, &DocId::Meta),
                &meta,
            )
            .unwrap();
        assert!(target.groups.lock()[&gid].state.is_member(me.id.did()));
        assert!(
            !target.groups.lock()[&gid]
                .state
                .is_host(me.id.did(), &target.dev)
        );
        let (update, _) = source
            .own_content_export(&gid, &DocId::Day(day.clone()), &VersionVector::default())
            .unwrap()
            .unwrap();
        let name = contents::doc_name(&gid, &DocId::Day(day.clone()));
        target.store.fail_next_append_for_test();
        assert!(matches!(
            node.open().group.group_own_content_apply(me.epoch, &remote, &name, &update),
            Err(node_runtime::Error::Io(_))
        ));
        assert!(!target.groups.lock()[&gid].meta.owed_days.contains(&day));
        let accepted = node
            .open().group
            .group_own_content_apply(me.epoch, &remote, &name, &update)
            .unwrap();
        assert!(target.groups.lock()[&gid].meta.owed_days.contains(&day));
        assert!(
            target
                .store
                .groups()
                .unwrap()
                .into_iter()
                .find(|(g, _)| g == &gid)
                .unwrap()
                .1
                .owed_days
                .contains(&day)
        );
        // Acknowledging only the laptop's own writer (absent here) must NOT clear
        // the phone-origin obligation, including after retry of a failed append.
        node.open().group.group_ack_for_durability_test(
            &target,
            &gid,
            "did:synthetic:host",
            &[9; 32],
            &DocId::Day(day.clone()),
            &VersionVector::default().encode(),
        );
        assert!(target.groups.lock()[&gid].meta.owed_days.contains(&day));
        node.open().group.group_ack_for_durability_test(
            &target,
            &gid,
            "did:synthetic:host",
            &[9; 32],
            &DocId::Day(day.clone()),
            &accepted,
        );
        assert!(!target.groups.lock()[&gid].meta.owed_days.contains(&day));
        assert!(
            !target
                .store
                .groups()
                .unwrap()
                .into_iter()
                .find(|(g, _)| g == &gid)
                .unwrap()
                .1
                .owed_days
                .contains(&day)
        );
    })
    .await
    .unwrap();
}
