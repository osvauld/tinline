//! A Loro doc that own devices sync whole (`tinline/self/1`), named on the wire: the account doc
//! (`accdoc.rs`) and the call log (`callsdoc.rs`). Load, snapshot, and export/import by version.

use loro::{ExportMode, LoroDoc, LoroValue, VersionVector};

use crate::Error;

pub struct Doc {
    pub(super) doc: LoroDoc,
    peer: u64,
}

pub(super) fn io<E: std::fmt::Display>(e: E) -> Error {
    Error::Io(e.to_string())
}

pub(super) fn entries(v: &LoroValue) -> Vec<(String, &LoroValue)> {
    match v {
        LoroValue::Map(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn field<'a>(v: &'a LoroValue, k: &str) -> Option<&'a LoroValue> {
    match v {
        LoroValue::Map(m) => m.get(k),
        _ => None,
    }
}

pub(super) fn text(v: &LoroValue) -> Option<String> {
    match v {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

impl Doc {
    /// `name` is the doc's wire name; the Loro peer id is derived from it and the device.
    pub fn new(device: &[u8; 32], name: &str) -> Self {
        let peer = crate::chat::doc::peer_id(device, name);
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id");
        Self { doc, peer }
    }

    pub fn load(device: &[u8; 32], name: &str, snapshot: Option<&[u8]>) -> Result<Self, Error> {
        let d = Self::new(device, name);
        if let Some(b) = snapshot {
            d.doc.import(b).map_err(io)?;
            d.doc.set_peer_id(d.peer).expect("peer id");
        }
        Ok(d)
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.doc.export(ExportMode::Snapshot).expect("snapshot export")
    }

    pub fn vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// The ops the peer (at `theirs`) lacks; `None` if there are none.
    pub fn export_since(&self, theirs: &VersionVector) -> Option<(Vec<u8>, VersionVector)> {
        let mine = self.vv();
        if mine.partial_cmp(theirs).is_some_and(|o| o != std::cmp::Ordering::Greater) {
            return None;
        }
        let bytes = self.doc.export(ExportMode::updates(theirs)).ok()?;
        Some((bytes, mine))
    }

    /// Imports a batch; whether the doc grew. A batch with ops whose dependencies are missing is
    /// refused (nothing is applied).
    pub fn import(&self, bytes: &[u8]) -> Result<bool, Error> {
        let before = self.vv();
        let status = self.doc.import(bytes).map_err(io)?;
        if status.pending.is_some() {
            return Err(Error::Protocol("update with missing dependencies".into()));
        }
        Ok(self.vv() != before)
    }
}
