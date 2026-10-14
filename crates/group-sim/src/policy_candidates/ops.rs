use super::*;

pub(super) const META_DOMAIN: &[u8] = b"tinline/group-sim/meta/not-production/v1\0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Change {
    Add {
        member: String,
        admin: bool,
        expires_at: Option<u64>,
    },
    Remove {
        member: String,
    },
    // Epoch-cut candidate only: complete member/admin lists for the new epoch plus the IDs of
    // previous-epoch posts the issuer closes into history (a production cut would be a VV).
    Epoch {
        number: u64,
        members: BTreeSet<String>,
        admins: BTreeSet<String>,
        cut: BTreeSet<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct MetaBody {
    pub(super) group: String,
    pub(super) issuer: String,
    pub(super) parents: BTreeSet<String>,
    pub(super) seen_posts: BTreeSet<String>,
    pub(super) at: u64,
    pub(super) change: Change,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MetaOp {
    pub(super) body: MetaBody,
    pub(super) signature: Vec<u8>,
}
impl MetaOp {
    pub(super) fn bytes(body: &MetaBody) -> Vec<u8> {
        [META_DOMAIN, &serde_json::to_vec(body).unwrap()].concat()
    }
    pub(super) fn issue(
        issuer: &Identity,
        group: &str,
        parents: &[&MetaOp],
        seen: &[&Envelope],
        at: u64,
        change: Change,
    ) -> Self {
        let body = MetaBody {
            group: group.into(),
            issuer: issuer.did().into(),
            parents: parents.iter().map(|p| p.id()).collect(),
            seen_posts: seen.iter().map(|e| e.id()).collect(),
            at,
            change,
        };
        let signature = issuer.sign(&Self::bytes(&body)).to_vec();
        Self { body, signature }
    }
    pub(super) fn id(&self) -> String {
        blake3::hash(&Self::bytes(&self.body)).to_hex().to_string()
    }
    pub(super) fn verify(&self, group: &str) -> bool {
        let Some(key) = public_key_from_did(&self.body.issuer) else {
            return false;
        };
        let Ok(sig) = <&[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        self.body.group == group && verify(&key, &Self::bytes(&self.body), sig)
    }
    pub(super) fn target(&self) -> Option<&str> {
        match &self.body.change {
            Change::Add { member, .. } | Change::Remove { member } => Some(member),
            Change::Epoch { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Header {
    pub(super) frontier: BTreeSet<String>,
    pub(super) at: u64,
    pub(super) epoch: Option<String>,
    pub(super) msg: String,
}
pub(super) fn header_text(frontier: &[&MetaOp], at: u64, epoch: Option<&MetaOp>, msg: &str) -> String {
    serde_json::to_string(&Header {
        frontier: frontier.iter().map(|m| m.id()).collect(),
        at,
        epoch: epoch.map(|e| e.id()),
        msg: msg.into(),
    })
    .unwrap()
}
pub(super) fn header(e: &Envelope) -> Option<Header> {
    serde_json::from_str(&e.body.text).ok()
}
pub(super) fn post(
    a: &mut Author,
    frontier: &[&MetaOp],
    at: u64,
    epoch: Option<&MetaOp>,
    msg: &str,
) -> Envelope {
    a.post(&header_text(frontier, at, epoch, msg))
}
// A fresh device chain of the same DID (reuses G-H's `second_device_envelope`).
pub(super) fn second_device_post(
    owner: &Author,
    seed: u8,
    frontier: &[&MetaOp],
    at: u64,
    epoch: Option<&MetaOp>,
    msg: &str,
) -> Envelope {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let writer = peer(&key.verifying_key().to_bytes(), &owner.doc_name);
    second_device_envelope(owner, &key, writer, &header_text(frontier, at, epoch, msg))
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(super) struct Members(pub(super) BTreeMap<String, (bool, Option<u64>)>);
impl Members {
    pub(super) fn genesis(creator: &str) -> Self {
        Self([(creator.to_string(), (true, None))].into())
    }
    pub(super) fn has(&self, did: &str) -> bool {
        self.0.contains_key(did)
    }
    pub(super) fn is_admin(&self, did: &str) -> bool {
        self.0.get(did).is_some_and(|m| m.0)
    }
    pub(super) fn expiry(&self, did: &str) -> Option<Option<u64>> {
        self.0.get(did).map(|m| m.1)
    }
    pub(super) fn apply(&mut self, op: &MetaOp, creator: &str) {
        match &op.body.change {
            Change::Add {
                member,
                admin,
                expires_at,
            } => {
                self.0.insert(member.clone(), (*admin, *expires_at));
            }
            Change::Remove { member } if member != creator => {
                self.0.remove(member);
            }
            _ => {}
        }
    }
}
pub(super) fn authorized(op: &MetaOp, s: &Members, creator: &str) -> bool {
    match &op.body.change {
        Change::Add { .. } => s.is_admin(&op.body.issuer),
        Change::Remove { member } => s.is_admin(&op.body.issuer) && member != creator,
        Change::Epoch { .. } => false,
    }
}

/// How concurrent membership ops on the same member resolve (D03).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Resolve {
    /// Maximal concurrent Add vs Remove on a member: removed. Each op is authorized at its
    /// OWN signed frontier.
    RemoveWins,
    /// Maximal concurrent Add vs Remove: present (OR-set style). Authorized at own frontier.
    AddWins,
    /// Deterministic fold: topological order, ready ops taken junior-first (issuer power
    /// ascending: member 0, admin 1, creator 2; ties by op ID) so seniors act last; each op is
    /// authorized against the FOLDED state at that point, not its own frontier.
    Priority,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Membership {
    pub(super) resolve: Resolve,
    /// Drop Adds whose issuer is not an admin in the evaluated state (fixpoint).
    pub(super) cascade: bool,
}
pub(super) const REMOVE_WINS: Membership = Membership {
    resolve: Resolve::RemoveWins,
    cascade: false,
};

/// How a post's validity is decided (D01, D02, D04, D05).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PostRule {
    /// Author is a member at the post's own signed meta frontier.
    SignedFrontier,
    /// Decided ONCE, when the post's references are present, against the receiver's whole
    /// current membership view; never revisited.
    ArrivalView,
    /// SignedFrontier AND no authorized removal of the author that is concurrent with the post
    /// (removal not in the post's frontier, post not in the remover's seen content).
    RemovalWinsConcurrent,
    /// Membership linearized by issuer-claimed `at`; post valid iff member at its claimed `at`.
    AuthorTimestamp,
    /// Decided ONCE at arrival: member at frontier AND receiver's local clock < grant expiry.
    LocalClockExpiry,
    /// Member at frontier AND author-claimed `at` < grant expiry.
    TimestampExpiry,
    /// Post names an epoch on the winning epoch chain whose member list includes the author,
    /// and either that epoch is the tip or the next winning epoch's cut names the post.
    EpochCut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Accepted,
    Rejected,
    Pending,
}
#[derive(Debug, PartialEq)]
pub(super) enum Admit {
    Stored,
    Pending,
    Duplicate,
    Invalid,
}
#[derive(Clone)]
pub(super) enum Ev {
    Meta(MetaOp),
    Post(Envelope),
}

// Speed-up for the all-orders loops: remember only POSITIVE signature checks, keyed by the
// exact signed bytes + signature + expected group.
pub(super) fn verify_cached(op: &MetaOp, group: &str) -> bool {
    thread_local! {
        static VALID: RefCell<BTreeSet<Vec<u8>>> = const { RefCell::new(BTreeSet::new()) };
    }
    let key = [
        MetaOp::bytes(&op.body),
        op.signature.clone(),
        group.as_bytes().to_vec(),
    ]
    .concat();
    if VALID.with(|v| v.borrow().contains(&key)) {
        return true;
    }
    let ok = op.verify(group);
    if ok {
        VALID.with(|v| v.borrow_mut().insert(key));
    }
    ok
}
