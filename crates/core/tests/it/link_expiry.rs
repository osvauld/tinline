//! A QR that was not scanned in time is dead. An isolated child process shortens its life.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use p2pcore::{LinkEvents, Node};

use crate::support;
use support::*;

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

fn node(tag: &str) -> (Arc<Node>, mpsc::Receiver<String>, TempDir) {
    let dir = TempDir::new("expiry", tag);
    let (tx, rx) = mpsc::channel();
    let n = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    n.set_link_events(Arc::new(L(Mutex::new(tx))));
    (n, rx, dir)
}

#[test]
fn an_expired_qr_is_refused() {
    if !support::with_env(&[("P2P_LINK_TTL_SECS", "2")]) {
        return;
    }
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
    drop((d1, d2));
}
