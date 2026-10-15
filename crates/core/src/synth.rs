//! Test-only hooks for the year-scale measurements (`docs/scale-year.md`): synthetic peers,
//! contacts, 1:1 messages and groups written through the real code paths with injected times,
//! and a synchronous search reconcile. `#[doc(hidden)]`, like the other `*_for_test` hooks;
//! the product never calls them.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use loro::VersionVector;

use crate::Error;
use crate::chat::doc::FileRef;
use crate::group::doc::GroupDoc;
use crate::group::record::Body;
use crate::node::Node;

type Peers = Mutex<HashMap<String, (identity::Identity, [u8; 32])>>;
static PEERS: OnceLock<Peers> = OnceLock::new();

pub(crate) fn peer(did: &str) -> Option<(identity::Identity, [u8; 32])> {
    PEERS.get()?.lock().ok()?.get(did).cloned()
}

pub(crate) struct SMember {
    pub did: String,
    pub acct: identity::Identity,
    pub dev: [u8; 32],
    /// The member's writing replica of the meta doc.
    pub doc: GroupDoc,
    /// ... and of the day docs it posted in.
    pub days: HashMap<String, GroupDoc>,
}

/// A group being built (see `Node::synth_group_new_for_test`).
#[doc(hidden)]
pub struct SynthGroup {
    pub(crate) gid: String,
    /// Where the members' batches meet: the meta doc and one doc per day.
    pub(crate) hub: GroupDoc,
    pub(crate) day_hubs: HashMap<String, GroupDoc>,
    pub(crate) members: Vec<SMember>,
    /// What went through `apply` already, per doc.
    pub(crate) sent: VersionVector,
    pub(crate) day_sent: HashMap<String, VersionVector>,
}

/// A remote member's writing doc (see `Node::synth_remote_open_for_test`).
#[doc(hidden)]
pub struct SynthRemote {
    pub(crate) gid: String,
    pub(crate) m: SMember,
}

impl SynthGroup {
    pub fn gid(&self) -> String {
        self.gid.clone()
    }
    pub fn member_did(&self, idx: usize) -> String {
        self.members[idx].did.clone()
    }
}

impl Node {
    /// A fresh recovery phrase.
    #[doc(hidden)]
    pub fn synth_phrase_for_test(&self) -> String {
        identity::generate().1.to_string()
    }

    /// Registers the identity of `phrase` as a synthetic peer (its device value is derived from
    /// the phrase, so it is the same in every process); returns its DID.
    #[doc(hidden)]
    pub fn synth_peer_for_test(&self, phrase: String) -> Result<String, Error> {
        let id = identity::recover(&phrase).map_err(|_| Error::BadPhrase)?;
        let dev = *blake3::hash(phrase.as_bytes()).as_bytes();
        let did = id.did().to_string();
        PEERS.get_or_init(Default::default).lock().unwrap().insert(did.clone(), (id, dev));
        Ok(did)
    }

    /// Makes the registered peer `did` a contact, through the real hello/welcome exchange, in
    /// process (no network).
    #[doc(hidden)]
    pub fn synth_contact_for_test(&self, did: String, name: String) -> Result<(), Error> {
        let (pid, pdev) = peer(&did).ok_or(Error::NotFound)?;
        let pubdev = cryptography::signature::public_key(&pdev);
        let inner = self.inner.clone();
        self.block_on(async move {
            let me = inner.me()?;
            let now = crate::node::now();
            let ticket = proto::issue_contact_ticket(&pid, pubdev, &name, None, now, 3600);
            let policy = crate::caps::policy_for(&inner.shared.lock().state, &did, crate::ContactOrigin::Ticket);
            let (hello, pending) = proto::contact_hello_with(&me.id, me.device, &ticket, &me.profile.name, now * 1000, crate::node::GRANT_TTL, policy)?;
            let (welcome, _) = proto::accept_contact_hello_with(&pid, pubdev, &hello, me.device, now * 1000, &proto::Unrecorded, crate::node::GRANT_TTL, |_| policy)?;
            let new = proto::accept_contact_welcome(&me.id, &pending, &welcome, pubdev, now * 1000)?;
            inner.save_contact(me.epoch, new, None, true, true).await?;
            Ok(())
        })?
    }

    /// One 1:1 message at unix ms `at`, authored by us or by the contact; `file`: (name, size)
    /// of an attachment whose metadata is recorded. Returns the message id.
    #[doc(hidden)]
    pub fn synth_dm_for_test(&self, with: String, author: String, text: String, at: i64, file: Option<(String, u64)>) -> Result<String, Error> {
        let inner = self.inner.clone();
        let file = file.map(|(name, size)| FileRef {
            hash: hex::encode(cryptography::rng::bytes::<32>()),
            key: hex::encode(cryptography::rng::bytes::<32>()),
            name,
            size,
            mime: "image/jpeg".into(),
            kind: "file".into(),
            duration_ms: 0,
            waveform: Vec::new(),
            width: 0,
            height: 0,
            preview: String::new(),
        });
        self.block_on(async move { inner.synth_dm(&with, &author, text, at, file).await })?
    }

    #[doc(hidden)]
    pub fn synth_group_new_for_test(&self, name: String) -> Result<SynthGroup, Error> {
        self.inner.synth_group_new(&name)
    }

    #[doc(hidden)]
    pub fn synth_group_add_for_test(&self, g: &mut SynthGroup, did: String, name: String, at: i64) -> Result<usize, Error> {
        self.inner.synth_group_add(g, &did, &name, at)
    }

    /// Buffers a post by member `idx`.
    #[doc(hidden)]
    pub fn synth_group_post_for_test(&self, g: &mut SynthGroup, idx: usize, at: i64, text: String) -> Result<(), Error> {
        self.inner.synth_group_write(g, idx, at, Body::Post { text, reply_to: None })
    }

    #[doc(hidden)]
    pub fn synth_group_flush_for_test(&self, g: &mut SynthGroup) -> Result<(), Error> {
        self.inner.synth_group_flush(g)
    }

    /// Only the meta doc's and/or only the day docs' buffered records go through `apply`.
    #[doc(hidden)]
    pub fn synth_group_flush_part_for_test(&self, g: &mut SynthGroup, meta: bool, days: bool) -> Result<(), Error> {
        self.inner.synth_group_flush_part(g, meta, days)
    }

    #[doc(hidden)]
    pub fn synth_remote_open_for_test(&self, gid: String, did: String) -> Result<SynthRemote, Error> {
        self.inner.synth_remote_open(&gid, &did)
    }

    /// A remote member's post arrives; returns (apply, after) times.
    #[doc(hidden)]
    pub fn synth_remote_post_for_test(&self, r: &mut SynthRemote, text: String, at: i64) -> Result<(Duration, Duration), Error> {
        self.inner.synth_remote_post(r, &text, at)
    }

    /// Switch for the background search reconcile (on open); off = the test calls
    /// `search_reconcile_now_for_test` itself.
    #[doc(hidden)]
    pub fn search_background_reconcile_for_test(&self, on: bool) {
        crate::search::api::NO_BG_RECONCILE.store(!on, std::sync::atomic::Ordering::SeqCst);
    }

    /// Runs the search reconcile now and waits; returns (1:1 part, groups part).
    #[doc(hidden)]
    pub fn search_reconcile_now_for_test(&self) -> Result<(Duration, Duration), Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.search_reconcile_now().await })?
    }
}
