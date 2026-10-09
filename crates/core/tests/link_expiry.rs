//! A QR that was not scanned in time is dead. Own binary: it shortens the QR's life for the
//! whole process.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use p2pcore::{CallInfo, CallState, LinkEvents, Node, NodeEvents, NodeStatus};

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

struct L(Mutex<mpsc::Sender<String>>);
impl LinkEvents for L {
    fn on_link_code(&self, _: String, _: String) {
        let _ = self.0.lock().unwrap().send("code".into());
    }
    fn on_link_done(&self, _: String, _: String) {}
    fn on_link_failed(&self, r: String) {
        let _ = self.0.lock().unwrap().send(format!("failed:{r}"));
    }
    fn on_devices_changed(&self) {}
    fn on_history_changed(&self) {}
    fn on_unlinked(&self, _: String) {}
}

fn node(tag: &str) -> (Arc<Node>, mpsc::Receiver<String>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("p2pcore-expiry-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = mpsc::channel();
    let n = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    n.set_link_events(Arc::new(L(Mutex::new(tx))));
    (n, rx, dir)
}

#[test]
fn an_expired_qr_is_refused() {
    unsafe { std::env::set_var("P2P_LINK_TTL_SECS", "2") };
    let (e, erx, d1) = node("e");
    e.create_identity("e".into(), "pw".into()).unwrap();
    e.start().unwrap();
    let (n, nrx, d2) = node("n");
    let qr = e.link_show_qr().unwrap();
    std::thread::sleep(Duration::from_secs(3));
    // E reported the expiry itself and is free again.
    assert_eq!(erx.recv_timeout(Duration::from_secs(5)).unwrap(), "failed:expired");
    n.link_new_scan(qr, "Pixel".into()).unwrap();
    let got = nrx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(got.starts_with("failed:"), "{got}");
    assert!(!n.has_identity());
    e.link_show_qr().unwrap();
    e.link_cancel();
    e.stop();
    let _ = (std::fs::remove_dir_all(d1), std::fs::remove_dir_all(d2));
}
