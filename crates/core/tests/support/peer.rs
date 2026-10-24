//! A test install: a `Node` with its data directory, event channels and identity.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use p2pcore::Node;

use super::{Ev, LEv, LTap, PASS, Tap, TempDir, online};

/// Opened node plus its event receivers and log lines.
pub type Opened = (
    Arc<Node>,
    Receiver<Ev>,
    Receiver<LEv>,
    Arc<Mutex<Vec<String>>>,
);

/// How `Peer::link_fresh` links a new install.
pub struct LinkAs {
    /// Device label the new install announces.
    pub label: &'static str,
    /// Passphrase set on the new install once linked.
    pub new_pass: &'static str,
    /// Also wait for the existing install's own "link done" event.
    pub wait_existing_done: bool,
}

impl Default for LinkAs {
    fn default() -> LinkAs {
        LinkAs {
            label: "Pixel",
            new_pass: "n-pass",
            wait_existing_done: false,
        }
    }
}

/// One test file's identity: the tag that names its temp directories. Replaces the
/// `peer`/`pair`/`link_fresh` wrappers every file used to re-roll: `const S: Suite = Suite("g2");`.
#[derive(Clone, Copy)]
pub struct Suite(pub &'static str);

impl Suite {
    /// A committed identity, started and online (`Peer::new`).
    pub fn peer(self, name: &str) -> Peer {
        Peer::new(self.0, name)
    }

    /// A committed identity, started (`online`) or not (`Peer::committed`).
    pub fn committed(self, name: &str, started: bool) -> Peer {
        Peer::committed(self.0, name, started)
    }

    /// As `committed`, with noisy link events (`Peer::committed_with(.., true)`).
    pub fn noisy(self, name: &str, started: bool) -> Peer {
        Peer::committed_with(self.0, name, started, true)
    }

    /// `a` and `b`, each a contact of the other (`Peer::pair`).
    pub fn pair(self, tag: &str) -> (Peer, Peer) {
        Peer::pair(self.0, tag)
    }

    /// `e` links a fresh install, default `LinkAs`.
    pub fn link_fresh(self, e: &Peer, name: &str) -> Peer {
        e.link_fresh(self.0, name, LinkAs::default())
    }

    /// As `link_fresh`, announcing the device label "Desk".
    pub fn link_desk(self, e: &Peer, name: &str) -> Peer {
        e.link_fresh(
            self.0,
            name,
            LinkAs {
                label: "Desk",
                ..LinkAs::default()
            },
        )
    }

    /// As `link_fresh`, also waiting for the existing install's own "link done".
    pub fn link_both_done(self, e: &Peer, name: &str) -> Peer {
        e.link_fresh(
            self.0,
            name,
            LinkAs {
                wait_existing_done: true,
                ..LinkAs::default()
            },
        )
    }
}

pub struct Peer {
    pub node: Arc<Node>,
    pub rx: Receiver<Ev>,
    pub lrx: Receiver<LEv>,
    pub logs: Arc<Mutex<Vec<String>>>,
    pub did: String,
    pub phrase: String,
    /// The passphrase the node is currently locked with.
    pub pass: String,
    /// Link events also report device / history / unlink changes (as `LEv::Other`).
    pub noisy_link: bool,
    pub dir: TempDir,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.node.stop();
        // `dir` is removed after this, when the fields drop.
    }
}

impl Peer {
    /// Opens a node on `dir` with fresh event channels.
    pub fn open(dir: &Path, noisy_link: bool) -> Opened {
        let (tx, rx) = mpsc::channel();
        let (ltx, lrx) = mpsc::channel();
        let logs = Arc::new(Mutex::new(Vec::new()));
        let node = Node::new(
            dir.to_string_lossy().into(),
            Arc::new(Tap::new(tx, logs.clone())),
        )
        .unwrap();
        // Tests that are not about contact requests: every good hello is welcomed at once.
        node.auto_accept_contacts_for_test(true);
        node.set_link_events(Arc::new(if noisy_link {
            LTap::noisy(ltx)
        } else {
            LTap::new(ltx)
        }));
        (node, rx, lrx, logs)
    }

    /// An empty install in a unique temporary directory: no identity, not started.
    pub fn blank(prefix: &str, name: &str) -> Peer {
        Peer::blank_with(prefix, name, false)
    }

    pub fn blank_with(prefix: &str, name: &str, noisy_link: bool) -> Peer {
        let dir = TempDir::new(prefix, name);
        let (node, rx, lrx, logs) = Peer::open(&dir, noisy_link);
        Peer {
            node,
            rx,
            lrx,
            logs,
            did: String::new(),
            phrase: String::new(),
            pass: PASS.into(),
            noisy_link,
            dir,
        }
    }

    /// A committed identity (passphrase `PASS`), started and online.
    pub fn new(prefix: &str, name: &str) -> Peer {
        Peer::committed(prefix, name, true)
    }

    /// A committed identity, started (`online`) or not.
    pub fn committed(prefix: &str, name: &str, started: bool) -> Peer {
        Peer::committed_with(prefix, name, started, false)
    }

    /// As `committed`, with `noisy_link` link events (see `LTap::noisy`).
    pub fn committed_with(prefix: &str, name: &str, started: bool, noisy_link: bool) -> Peer {
        let mut p = Peer::blank_with(prefix, name, noisy_link);
        p.commit(name, started);
        p
    }

    /// `create_identity` on a blank peer; fills `phrase` and `did`.
    pub fn commit(&mut self, name: &str, started: bool) {
        self.phrase = self.node.create_identity(name.into(), PASS.into()).unwrap();
        if started {
            online(&self.node);
        }
        self.did = self.node.profile().unwrap().did;
    }

    /// The same person on a new install: `restore_identity` from `phrase` (new device key),
    /// started and online.
    pub fn restored(prefix: &str, name: &str, phrase: &str) -> Peer {
        let mut p = Peer::blank(prefix, name);
        p.node
            .restore_identity(phrase.into(), name.into(), PASS.into())
            .unwrap();
        p.phrase = phrase.to_string();
        online(&p.node);
        p.did = p.node.profile().unwrap().did;
        p
    }

    /// `a` and `b`, each a contact of the other (b scanned a's ticket).
    pub fn pair(prefix: &str, tag: &str) -> (Peer, Peer) {
        let a = Peer::new(prefix, &format!("{tag}-a"));
        let b = Peer::new(prefix, &format!("{tag}-b"));
        b.node.add_contact(a.node.my_ticket().unwrap()).unwrap();
        (a, b)
    }

    /// `self` (existing, online) links a fresh install of its account for real (QR + code);
    /// returns it committed and online.
    pub fn link_fresh(&self, prefix: &str, name: &str, how: LinkAs) -> Peer {
        let mut n = Peer::blank_with(prefix, name, self.noisy_link);
        let qr = self.node.link_show_qr().unwrap();
        n.node.link_new_scan(qr, how.label.into()).unwrap();
        let code = |p: &Peer| loop {
            if let LEv::Code(c) = p
                .lrx
                .recv_timeout(Duration::from_secs(30))
                .expect("link code")
            {
                break c;
            }
        };
        assert_eq!(code(self), code(&n));
        self.node.link_approve(Some(PASS.into())).unwrap();
        let did = loop {
            if let LEv::Done(d) = n
                .lrx
                .recv_timeout(Duration::from_secs(30))
                .expect("link done")
            {
                break d;
            }
        };
        if how.wait_existing_done {
            loop {
                if let LEv::Done(_) = self
                    .lrx
                    .recv_timeout(Duration::from_secs(30))
                    .expect("link done")
                {
                    break;
                }
            }
        }
        n.node.set_passphrase(None, how.new_pass.into()).unwrap();
        n.pass = how.new_pass.into();
        online(&n.node);
        n.did = did;
        n.phrase = self.phrase.clone();
        n
    }

    /// Stops the node, drops it and opens the same data directory again (locked), with fresh
    /// channels. No pause: callers that need one add it themselves.
    pub fn reopen(&mut self) {
        self.node.stop();
        let (node, rx, lrx, logs) = Peer::open(&self.dir, self.noisy_link);
        drop(std::mem::replace(&mut self.node, node));
        self.rx = rx;
        self.lrx = lrx;
        self.logs = logs;
    }

    /// A process restart: `reopen` (no pause), unlock with `PASS`, start and wait online.
    pub fn restart(&mut self) {
        self.reopen_unlocked(PASS);
        online(&self.node);
    }

    /// `reopen`, then unlock with `pass`. Left stopped.
    pub fn reopen_unlocked(&mut self, pass: &str) {
        self.reopen();
        self.node.unlock(pass.into()).unwrap();
    }

    /// The forgotten-passphrase restore over the same directory: reopen, then restore the
    /// account from the phrase with the same name and passphrase "pass two". Left NOT started.
    /// Asserts the DID is kept and the device key is not. Returns (old, new) device keys.
    pub fn restore_in_place(&mut self) -> (String, String) {
        let name = self.node.profile().unwrap().name;
        let old_dev = self.node.device_key_for_test().unwrap();
        self.reopen();
        self.node
            .restore_identity(self.phrase.clone(), name, "pass two".into())
            .unwrap();
        assert_eq!(
            self.node.profile().unwrap().did,
            self.did,
            "restore keeps the DID"
        );
        let new_dev = self.node.device_key_for_test().unwrap();
        assert_ne!(new_dev, old_dev, "restore makes a fresh device key");
        (old_dev, new_dev)
    }

    /// Whether any log line contains `needle`.
    pub fn logged(&self, needle: &str) -> bool {
        self.logs.lock().unwrap().iter().any(|l| l.contains(needle))
    }
}

impl Peer {
    /// The log lines about refused batches.
    pub fn refusals(&self) -> Vec<String> {
        self.logs
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("rejected "))
            .cloned()
            .collect()
    }

    /// Whether this node refused any chat batch, history snapshot or own-device batch.
    pub fn refused_chat(&self) -> bool {
        [
            "rejected chat batch",
            "rejected history",
            "rejected own-device batch",
        ]
        .iter()
        .any(|n| self.logged(n))
    }

    /// This install's device key.
    pub fn key(&self) -> String {
        self.node.device_key_for_test().unwrap()
    }

    /// The device keys this node will dial for a contact.
    pub fn dials(&self, other: &Peer) -> Vec<String> {
        self.node.contact_devices_for_test(other.did.clone())
    }

    /// The same device list, named for device-list characterization.
    pub fn devices_of(&self, other: &Peer) -> Vec<String> {
        self.dials(other)
    }

    /// The contact record, required to exist.
    pub fn contact(&self, did: &str) -> p2pcore::Contact {
        self.node
            .contacts()
            .into_iter()
            .find(|c| c.did == did)
            .expect("a contact")
    }

    /// Contact state for before/after comparisons.
    pub fn dump(&self) -> String {
        format!("{:?}", self.node.contacts())
    }
}
