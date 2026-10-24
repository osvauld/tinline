//! Local QUIC framing + actual group_read failure branch. This bypasses Hello
//! admission deliberately; it tests persistence/ack/fan-out, not membership setup.
use super::*;
use node_runtime::{CallInfo, CallState, NodeSink, NodeStatus};

pub(super) struct Quiet;
impl NodeSink for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

pub(super) struct TestDir(pub(super) std::path::PathBuf);
impl TestDir {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tinline-owned-durability-{}-{}",
            std::process::id(),
            hex::encode(proto::new_device_secret())
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        // Only this test's newly-created random directory, never shared outputs.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A group runtime over a bare node context in `directory`: no identity, nothing started (what
/// `Node::new` builds, without the app's callbacks).
pub(super) fn bare_rt(directory: &TestDir) -> Arc<GroupRt> {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let handle = tokio::runtime::Handle::try_current()
        .unwrap_or_else(|_| RT.get_or_init(|| tokio::runtime::Runtime::new().unwrap()).handle().clone());
    let accounts = account_model::accounts::AccountDirs::open(directory.0.clone()).unwrap();
    let ctx = Arc::new(NodeCtx::new(handle, accounts, node_runtime::Shared::new(), Arc::new(Quiet)));
    Arc::new(GroupRt::new(ctx, Default::default()))
}

async fn endpoint() -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .clear_ip_transports()
        .clear_relay_transports()
        .clear_address_lookup()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .alpns(vec![GROUP_ALPN.to_vec()])
        .bind()
        .await
        .unwrap()
}

fn session(id: u64, gid: &str, did: &str) -> (Arc<Sess>, mpsc::UnboundedReceiver<Out>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Arc::new(Sess {
            id,
            gid: gid.into(),
            did: did.into(),
            dev: [1; 32],
            ep: [1; 32],
            hosting: true,
            joining: false,
            open_join: false,
            show: true,
            tx,
            peer_vv: Default::default(),
            peer_days: Default::default(),
            inv_done: AtomicBool::new(false),
            receipt_vv: Default::default(),
            receipt_waiting: Default::default(),
        }),
        rx,
    )
}

#[tokio::test(flavor = "current_thread")]
async fn persistence_failure_in_group_reader_emits_no_ack_or_fanout() {
    tokio::time::timeout(Duration::from_secs(20), async {
        for doc in [DocId::Meta, day()] {
            let directory = TestDir::new();
            // No identity or Node::start: no production endpoints/discovery/service.
            let node = bare_rt(&directory);
            let f = Fixture::new();
            let update = if doc == DocId::Meta {
                f.meta_update()
            } else {
                f.post_update()
            };
            let gid = f.gid.clone();
            let before = f.core.doc_vv(&gid, &doc);
            let (sess, mut rx) = session(1, &gid, f.author.did());
            let (observer, mut observer_rx) = session(2, &gid, "did:synthetic:observer");
            let core = Arc::new(f.core);
            core.sessions.lock().insert(sess.id, sess.clone());
            core.sessions.lock().insert(observer.id, observer);
            core.store.fail_next_append_for_test();

            let (client, server) = (endpoint().await, endpoint().await);
            let (outgoing, incoming) =
                tokio::join!(client.connect(server.addr(), GROUP_ALPN), async {
                    server.accept().await.unwrap().await
                });
            let (client_conn, server_conn) = (outgoing.unwrap(), incoming.unwrap());
            let writer = async {
                let (send, _) = client_conn.open_bi().await.unwrap();
                let mut writer = Writer(send);
                writer
                    .send(&GroupMsg::Update {
                        doc: doc.clone(),
                        update,
                    })
                    .await
                    .unwrap();
                writer.0.finish().unwrap();
            };
            let reader = async {
                let (_, recv) = server_conn.accept_bi().await.unwrap();
                node
                    .group_read(&core, &sess, &mut Reader::new(recv))
                    .await
            };
            let (_, result) = tokio::join!(writer, reader);
            assert!(
                matches!(result, Err(Error::Io(_))),
                "storage failure is retryable, not a success or membership refusal"
            );
            assert!(
                rx.try_recv().is_err(),
                "no Ack/Refused emitted for failed storage"
            );
            assert!(
                observer_rx.try_recv().is_err(),
                "unpersisted data must not fan out"
            );
            assert_eq!(core.doc_vv(&gid, &doc), before);
            client_conn.close(0u32.into(), b"test complete");
            tokio::time::timeout(Duration::from_secs(3), server_conn.closed())
                .await
                .unwrap();
            client.close().await;
            server.close().await;
            drop(node);
            drop(directory);
        }
    })
    .await
    .expect("bounded local reader test");
}
