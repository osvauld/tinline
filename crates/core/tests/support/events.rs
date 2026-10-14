//! Event sinks for `Node` tests: a no-op `Quiet`, a call/log `Tap`, and the link-flow `LTap`.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use p2pcore::{CallInfo, CallState, LinkEvents, NodeEvents, NodeStatus};

/// Ignores every node event.
pub struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: CallInfo) {}
    fn on_call_state(&self, _: String, _: CallState) {}
    fn on_log(&self, _: String) {}
}

/// Ignores every link event.
pub struct NoLink;
impl LinkEvents for NoLink {
    fn on_link_code(&self, _: String, _: String) {}
    fn on_link_done(&self, _: String, _: String) {}
    fn on_link_failed(&self, _: String) {}
    fn on_devices_changed(&self) {}
    fn on_history_changed(&self) {}
    fn on_unlinked(&self, _: String) {}
}

#[derive(Debug, Clone)]
pub enum Ev {
    Incoming(CallInfo),
    State(String, CallState),
}

/// Forwards call events into a channel and keeps the log lines.
pub struct Tap {
    tx: Mutex<Sender<Ev>>,
    logs: Arc<Mutex<Vec<String>>>,
}

impl Tap {
    pub fn new(tx: Sender<Ev>, logs: Arc<Mutex<Vec<String>>>) -> Tap {
        Tap {
            tx: Mutex::new(tx),
            logs,
        }
    }
}

impl NodeEvents for Tap {
    fn on_status(&self, _: NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, c: CallInfo) {
        let _ = self.tx.lock().unwrap().send(Ev::Incoming(c));
    }
    fn on_call_state(&self, id: String, s: CallState) {
        let _ = self.tx.lock().unwrap().send(Ev::State(id, s));
    }
    fn on_log(&self, line: String) {
        self.logs.lock().unwrap().push(line);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LEv {
    Code(String),
    Done(String),
    Other,
}

/// Forwards link events into a channel. `noisy` also reports devices / history / unlink changes
/// as `LEv::Other`; otherwise those are ignored.
pub struct LTap {
    tx: Mutex<Sender<LEv>>,
    noisy: bool,
}

impl LTap {
    pub fn new(tx: Sender<LEv>) -> LTap {
        LTap {
            tx: Mutex::new(tx),
            noisy: false,
        }
    }

    pub fn noisy(tx: Sender<LEv>) -> LTap {
        LTap {
            tx: Mutex::new(tx),
            noisy: true,
        }
    }

    fn send(&self, e: LEv) {
        let _ = self.tx.lock().unwrap().send(e);
    }

    fn maybe(&self) {
        if self.noisy {
            self.send(LEv::Other);
        }
    }
}

impl LinkEvents for LTap {
    fn on_link_code(&self, code: String, _: String) {
        self.send(LEv::Code(code));
    }
    fn on_link_done(&self, did: String, _: String) {
        self.send(LEv::Done(did));
    }
    fn on_link_failed(&self, _: String) {
        self.send(LEv::Other);
    }
    fn on_devices_changed(&self) {
        self.maybe();
    }
    fn on_history_changed(&self) {
        self.maybe();
    }
    fn on_unlinked(&self, _: String) {
        self.maybe();
    }
}
