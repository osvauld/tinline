use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SignedOp {
    pub(super) body: OpBody,
    pub(super) sig: Vec<u8>,
}
impl SignedOp {
    pub(super) fn sign(id: &Identity, group: &str, grant: Option<MGrant>, nonce: u64, op: Op) -> Self {
        let body = OpBody {
            group: group.into(),
            issuer: id.did().into(),
            issuer_grant: grant,
            nonce,
            op,
        };
        let sig = id.sign(&signed(OP_DOMAIN, &body)).to_vec();
        Self { body, sig }
    }
    pub(super) fn verify(&self) -> bool {
        valid(&self.body.issuer, &signed(OP_DOMAIN, &self.body), &self.sig)
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    pub(super) fn id(&self) -> String {
        digest(self)
    }
}
// U5 revised + section 8b Q1 (decided): one DID-signed "blocked" notice, returned again on
// every later attempt. Wording and timing (8b Q2/Q3) are not modelled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BlockBody {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BlockNotice {
    pub(super) body: BlockBody,
    pub(super) sig: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refuse {
    Malformed,
    BadSignature,
    WrongAudience,
    WrongGroup,
    PermitInvalid,
    DeviceMismatch,
    Revoked,
    Stale,
    CapReached,
    NotAdmin,
    NotAppointedAfter,
    NotMember,
    UnknownHost,
    NotAddressed,
    NotHost,
}

// What every device (host or member) believes about the group's control plane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct View {
    pub(super) hosts: BTreeMap<String, HostRecord>, // "<did>#<slot>" -> accepted host record
    pub(super) revoked: BTreeSet<String>,           // revoked permit digests
    pub(super) ops: BTreeMap<String, SignedOp>,
    pub(super) flags: BTreeMap<String, BTreeSet<String>>, // target -> distinct admin flaggers
    pub(super) removed: BTreeSet<String>,
    pub(super) blocked_by: BTreeMap<String, BlockNotice>,
}

// ---- configuration (open policy as parameters) ---------------------------------------------

// G-FORK: cross-host writer-ID claim resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClaimRule {
    /// Each host binds writer -> DID on first accept (host_only.rs rule, per host).
    FirstAtHost,
    /// Hosts hold a new writer's posts until every host has seen every claim for it, then
    /// the lowest claim (blake3 of the signed seq-1 post) wins everywhere.
    LowestAfterAllHostsSaw,
}
// G-TIME: does a member keep its old group endpoint listening until every host acked?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Retention {
    None,
    UntilHostAck,
}
// G-AUTHORITY: which admins may remove a host ("newer admins can remove" — unconfirmed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RemovalRule {
    AnyAdmin,
    AdminAppointedAfterHost,
}
// G-AUTHORITY: maximum hosts a member accepts (user passed; not decided).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HostCap {
    Unbounded,
    Max(usize),
}
#[derive(Clone)]
pub(super) struct Cfg {
    pub(super) seed: u64,
    pub(super) hosts: Vec<usize>,
    pub(super) claim: ClaimRule,
    pub(super) retention: Retention,
    pub(super) removal: RemovalRule,
    pub(super) cap: HostCap,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            seed: 0x5EED_0002,
            hosts: vec![H1, H2],
            claim: ClaimRule::FirstAtHost,
            retention: Retention::None,
            removal: RemovalRule::AnyAdmin,
            cap: HostCap::Unbounded,
        }
    }
}

// ---- devices and world ---------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct HostState {
    pub(super) record: HostRecord,
    pub(super) directory: BTreeMap<String, EpNote>, // member device "<did>#<slot>" -> group note (U7)
    pub(super) held: BTreeMap<String, Post>,        // LowestAfterAllHostsSaw: unsettled writers' posts
    pub(super) claims: BTreeMap<u64, BTreeMap<String, String>>, // writer -> claim id -> DID
    pub(super) known_by: BTreeMap<String, BTreeSet<usize>>, // claim id -> hosts known to hold it
    pub(super) settled: BTreeMap<u64, String>,      // writer -> winning claim id
    pub(super) refusals: BTreeMap<String, Why>,     // post id -> why (told to the author device)
}
pub(super) struct Device {
    pub(super) id: Identity,
    pub(super) slot: u64, // random device ID / route slot; owner-replaceable, never key-derived
    pub(super) w: Writer,
    pub(super) board: Board,
    pub(super) view: View,
    pub(super) group_key: SigningKey,
    pub(super) my_note: EpNote,
    pub(super) private_key: SigningKey,
    pub(super) retained: Vec<String>,
    pub(super) acks: BTreeMap<String, u64>, // host key -> seq of my note that host reported holding
    pub(super) outbox: Vec<Post>,
    pub(super) sent: Vec<Post>,
    pub(super) refused: Vec<(Post, Why)>,
    pub(super) op_out: Vec<SignedOp>,
    pub(super) online: bool,
    pub(super) host: Option<HostState>,
    pub(super) private_book: BTreeMap<String, String>,
    pub(super) blocked: BTreeMap<String, BlockNotice>, // notices I sent (returned on later attempts)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteErr {
    SelfOffline,
    PeerOffline,
    EndpointGone,
    Partitioned,
    NoAddress,
    Lost,
    Refused,
    Removed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Path {
    MemberDialedHost,
    HostDialedMember,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Direct {
    Delivered,
    Blocked(Vec<u8>),
    Unreachable,
}
#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) to_ep: String,
    pub(super) kind: &'static str,
    pub(super) bytes: Vec<u8>,
}
pub(super) struct World {
    pub(super) cfg: Cfg,
    pub(super) rng: SimRng,
    pub(super) now: u64,
    pub(super) g: Genesis,
    pub(super) gid: String,
    pub(super) d: Vec<Device>,
    pub(super) grants: BTreeMap<String, MGrant>, // DID -> its admin/member grant (creator has none)
    pub(super) hosts: Vec<usize>,
    pub(super) rogue: BTreeSet<usize>, // hosts that ignore U9 removal (limitation fixture)
    pub(super) live: BTreeMap<String, usize>,
    pub(super) cut: BTreeSet<(usize, usize)>,
    pub(super) frames: Vec<Frame>,
}
