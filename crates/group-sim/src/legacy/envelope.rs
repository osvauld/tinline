use super::*;

pub(crate) const DOMAIN: &[u8] = b"tinline/group-simulation/not-production/v1\0";
pub(crate) const MAX_PENDING: usize = 16;

// LEGACY ENVELOPE FIXTURE (R1b). `Body.device`, the device-derived `peer` and the attestation
// encode the superseded device-key-signed content model; U4 revised + U16 replace them with
// DID-signed posts and an app-supplied writer ID (`host_only.rs`, `multi_host.rs`), and U18
// drops compatibility with the old shape. Tests in this file, `persistence.rs`,
// `policy_candidates.rs`, `reachability.rs`, `scheduler.rs` and `block_in_group.rs` use the
// envelope only as a carrier for properties that do not depend on how content is signed
// (relay, ordering, pending bounds, authority, persistence, policy candidates). R1b removed
// the tests that asserted the superseded property itself:
// - `attestation_must_bind_author_did_and_device_key_each_as_sole_cause` -> `host_only.rs`
//   `did_signed_posts_with_random_writer_ids_two_own_devices_converge` (DID key alone verifies,
//   no attestation) and `forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged`;
// - `second_device_cannot_write_under_another_devices_loro_peer_id` -> `host_only.rs`
//   `forged_tampered_wrong_group_and_replayed_posts_leave_state_unchanged` (ops under another
//   writer ID: `LoroMismatch`) and `cross_did_writer_collision_first_claim_at_host_wins_and_loser_redraws`.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Body {
    pub(crate) group: String,
    pub(crate) doc: String,
    pub(crate) author: String,
    pub(crate) device: [u8; 32],
    pub(crate) peer: u64,
    pub(crate) sequence: u64,
    pub(crate) previous: Option<String>,
    pub(crate) dependencies: BTreeSet<String>,
    pub(crate) permit: Option<Grant>,
    pub(crate) text: String,
    pub(crate) update: Vec<u8>,
}
#[derive(Clone)]
pub(crate) struct Envelope {
    pub(crate) body: Body,
    pub(crate) attestation: proto::SignedAttestation,
    pub(crate) signature: [u8; 64],
}
impl Envelope {
    pub(crate) fn bytes(&self) -> Vec<u8> {
        [DOMAIN, &serde_json::to_vec(&self.body).unwrap()].concat()
    }
    pub(crate) fn id(&self) -> String {
        blake3::hash(&self.bytes()).to_hex().to_string()
    }
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.body.peer, self.body.sequence)
    }
    pub(crate) fn value(&self) -> String {
        serde_json::to_string(&(&self.body.author, &self.body.text)).unwrap()
    }
}
// Provisional author adapter: peer IDs bind signing key + document, never a routing hint.
pub(crate) fn peer(key: &[u8; 32], doc: &str) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(DOMAIN);
    h.update(key);
    h.update(doc.as_bytes());
    u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap())
}
pub(crate) struct Author {
    pub(crate) id: Identity,
    pub(crate) key: SigningKey,
    pub(crate) attestation: proto::SignedAttestation,
    pub(crate) group: String,
    pub(crate) doc_name: String,
    pub(crate) doc: LoroDoc,
    pub(crate) sequence: u64,
    pub(crate) previous: Option<String>,
    pub(crate) observed: BTreeSet<String>,
    pub(crate) permit: Option<Grant>,
}
impl Author {
    pub(crate) fn new(seed: u8, group: &str) -> Self {
        let mnemonic = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
        let id = identity::recover(&mnemonic.to_string()).unwrap();
        let key = SigningKey::from_bytes(&[seed; 32]);
        let doc_name = format!("group/{group}/2026-10-01");
        let doc = LoroDoc::new();
        doc.set_peer_id(peer(&key.verifying_key().to_bytes(), &doc_name))
            .unwrap();
        let attestation = proto::attest(&id, key.verifying_key().to_bytes(), 0);
        Self {
            id,
            key,
            attestation,
            group: group.into(),
            doc_name,
            doc,
            sequence: 0,
            previous: None,
            observed: BTreeSet::new(),
            permit: None,
        }
    }
    pub(crate) fn configure(&mut self, group: &str, permit: Grant) {
        assert_eq!(self.sequence, 0);
        self.group = group.into();
        self.doc_name = format!("group/{group}/2026-10-01");
        self.doc = LoroDoc::new();
        self.doc
            .set_peer_id(peer(&self.key.verifying_key().to_bytes(), &self.doc_name))
            .unwrap();
        self.permit = Some(permit);
    }
    // Fixture-only: caller supplies an already validated original envelope.
    pub(crate) fn observe(&mut self, e: &Envelope) {
        let status = self.doc.import(&e.body.update).unwrap();
        assert!(status.pending.is_none());
        self.observed.insert(e.id());
        self.observed.extend(e.body.dependencies.iter().cloned());
        if let Some(p) = &e.body.previous {
            self.observed.insert(p.clone());
        }
    }
    pub(crate) fn post(&mut self, text: &str) -> Envelope {
        let before = self.doc.oplog_vv();
        self.sequence += 1;
        let p = peer(&self.key.verifying_key().to_bytes(), &self.doc_name);
        let value = serde_json::to_string(&(self.id.did(), text)).unwrap();
        self.doc
            .get_map("messages")
            .insert(&format!("{p}:{}", self.sequence), value)
            .unwrap();
        self.doc.commit();
        let mut e = Envelope {
            body: Body {
                group: self.group.clone(),
                doc: self.doc_name.clone(),
                author: self.id.did().into(),
                device: self.key.verifying_key().to_bytes(),
                peer: p,
                sequence: self.sequence,
                previous: self.previous.clone(),
                dependencies: self.observed.clone(),
                permit: self.permit.clone(),
                text: text.into(),
                update: self.doc.export(ExportMode::updates(&before)).unwrap(),
            },
            attestation: self.attestation.clone(),
            signature: [0; 64],
        };
        e.signature = self.key.sign(&e.bytes()).to_bytes();
        self.previous = Some(e.id());
        e
    }
}
