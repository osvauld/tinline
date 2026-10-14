use super::*;

pub(super) const NOTE_DOMAIN: &[u8] = b"tinline/group-sim/rotation-note/not-production/v1\0";
pub(super) const HOST: usize = 0;
pub(super) const A: usize = 1;
pub(super) const C: usize = 2;
pub(super) const D: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum NoteAudience {
    Group(String),
    Private,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct NoteClaims {
    pub(super) owner: String,
    pub(super) audience: NoteAudience,
    pub(super) endpoint: String,
    pub(super) seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RotationNote {
    pub(super) claims: NoteClaims,
    pub(super) signature: Vec<u8>,
}
impl RotationNote {
    pub(super) fn signed_bytes(claims: &NoteClaims) -> Vec<u8> {
        [NOTE_DOMAIN, &serde_json::to_vec(claims).unwrap()].concat()
    }
    pub(super) fn sign(id: &Identity, audience: NoteAudience, endpoint: &str, seq: u64) -> Self {
        let claims = NoteClaims {
            owner: id.did().into(),
            audience,
            endpoint: endpoint.into(),
            seq,
        };
        let signature = id.sign(&Self::signed_bytes(&claims)).to_vec();
        Self { claims, signature }
    }
    pub(super) fn verify(&self) -> bool {
        let Some(key) = public_key_from_did(&self.claims.owner) else {
            return false;
        };
        let Ok(sig) = <[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        verify(&key, &Self::signed_bytes(&self.claims), &sig)
    }
    pub(super) fn wire(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum Reject {
    Malformed,
    PrivateAudience,
    WrongGroup,
    NotMember,
    NotHost,
    BadSignature,
    Frozen,
    Replay,
    Stale,
    ForkSameSeq,
    Gap,
}
// Open gate G-HISTORY (Q3): may a receiver skip intermediate notes?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GapPolicy {
    AcceptGap,
    RequireContiguous,
}
// Open gate G-FORK (Q4.1): two different notes with the same (owner, seq).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ForkPolicy {
    KeepFirst,
    Freeze,
}
// Open gate G-HISTORY: what the host (or a forwarder) sends on catch-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CatchUp {
    FullHistory,
    LatestOnly,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Policy {
    pub(super) gap: GapPolicy,
    pub(super) fork: ForkPolicy,
    pub(super) catch_up: CatchUp,
}
// Invariant tests run under every candidate combination: they must not depend on the choice.
pub(super) fn all_policies() -> Vec<Policy> {
    let mut out = vec![];
    for gap in [GapPolicy::AcceptGap, GapPolicy::RequireContiguous] {
        for fork in [ForkPolicy::KeepFirst, ForkPolicy::Freeze] {
            for catch_up in [CatchUp::FullHistory, CatchUp::LatestOnly] {
                out.push(Policy {
                    gap,
                    fork,
                    catch_up,
                });
            }
        }
    }
    out
}

// Group-audience directory. The host's holds every member's notes; a member's (U7) holds only
// its own and the host's. Serializable so that rejection tests compare full bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct GroupView {
    pub(super) group: String,
    pub(super) roster: BTreeSet<String>,
    // U7: `Some({self, host})` on a member, `None` on the host.
    pub(super) accept_only: Option<BTreeSet<String>>,
    pub(super) history: BTreeMap<String, Vec<RotationNote>>,
    pub(super) frozen: BTreeSet<String>,
}
impl GroupView {
    pub(super) fn current(&self, did: &str) -> Option<&RotationNote> {
        self.history.get(did).and_then(|h| h.last())
    }
    pub(super) fn route(&self, did: &str) -> Option<&RotationNote> {
        if self.frozen.contains(did) {
            return None;
        }
        self.current(did)
    }
    pub(super) fn accept(&mut self, policy: Policy, bytes: &[u8]) -> Result<RotationNote, Reject> {
        let note: RotationNote = serde_json::from_slice(bytes).map_err(|_| Reject::Malformed)?;
        let c = &note.claims;
        match &c.audience {
            NoteAudience::Private => return Err(Reject::PrivateAudience),
            NoteAudience::Group(g) if g != &self.group => return Err(Reject::WrongGroup),
            NoteAudience::Group(_) => {}
        }
        if !self.roster.contains(&c.owner) {
            return Err(Reject::NotMember);
        }
        if self
            .accept_only
            .as_ref()
            .is_some_and(|o| !o.contains(&c.owner))
        {
            return Err(Reject::NotHost);
        }
        if !note.verify() {
            return Err(Reject::BadSignature);
        }
        if self.frozen.contains(&c.owner) {
            return Err(Reject::Frozen);
        }
        let history = self.history.get(&c.owner);
        if history.is_some_and(|h| h.contains(&note)) {
            return Err(Reject::Replay);
        }
        let last_seq = history.and_then(|h| h.last()).map(|n| n.claims.seq);
        match last_seq {
            Some(last) if c.seq < last => return Err(Reject::Stale),
            Some(last) if c.seq == last => {
                if policy.fork == ForkPolicy::Freeze {
                    // Candidate: the fork itself is state (freezes the path).
                    self.frozen.insert(c.owner.clone());
                }
                return Err(Reject::ForkSameSeq);
            }
            _ => {}
        }
        if policy.gap == GapPolicy::RequireContiguous && c.seq != last_seq.unwrap_or(0) + 1 {
            return Err(Reject::Gap);
        }
        self.history
            .entry(c.owner.clone())
            .or_default()
            .push(note.clone());
        Ok(note)
    }
    // One owner's notes as offered on a retry/catch-up (G-HISTORY).
    pub(super) fn export_owner(&self, mode: CatchUp, owner: &str) -> Vec<Vec<u8>> {
        let h = self.history.get(owner).map(Vec::as_slice).unwrap_or(&[]);
        match mode {
            CatchUp::FullHistory => h.iter().map(RotationNote::wire).collect(),
            CatchUp::LatestOnly => h.last().map(RotationNote::wire).into_iter().collect(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum RouteErr {
    SelfOffline,
    NoBinding,
    EndpointGone,
}
#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) bytes: Vec<u8>,
}
pub(super) struct Node {
    pub(super) name: &'static str,
    pub(super) author: Author,
    pub(super) did: String,
    pub(super) view: GroupView,
    pub(super) group_seq: u64,
    pub(super) private_seq: u64,
    pub(super) private_endpoint: String,
    pub(super) private_contacts: BTreeSet<String>,
    // Personal block list: private-audience only (U5). Never consulted by group code.
    pub(super) blocked: BTreeSet<String>,
    pub(super) online: bool,
    // Bytes this node sent in its role as group host (relay + catch-up).
    pub(super) relayed: Vec<Vec<u8>>,
}
pub(super) struct World {
    pub(super) policy: Policy,
    pub(super) group: String,
    pub(super) nodes: Vec<Node>,
    pub(super) live: BTreeMap<String, String>, // listening endpoint -> owner DID
    pub(super) wire: Vec<Frame>,               // every frame sent over a group-audience link
    pub(super) private_wire: Vec<Frame>,       // private-audience notes, sent directly to contacts
    pub(super) private_endpoints_ever: BTreeSet<String>,
}
pub(super) fn world(policy: Policy) -> World {
    let (g, a, b, c) = signed_fixture();
    let d = Author::new(4, &g.id());
    let group = g.id();
    let authors = vec![("host", b), ("alice", a), ("carol", c), ("dave", d)];
    let roster: BTreeSet<String> = authors
        .iter()
        .map(|(_, x)| x.id.did().to_string())
        .collect();
    let mut w = World {
        policy,
        group: group.clone(),
        nodes: vec![],
        live: BTreeMap::new(),
        wire: vec![],
        private_wire: vec![],
        private_endpoints_ever: BTreeSet::new(),
    };
    let mut bootstrap = vec![];
    let host_did = authors[HOST].1.id.did().to_string();
    for (name, author) in authors {
        let did = author.id.did().to_string();
        let ep = format!("grp-{name}-1");
        bootstrap.push(
            RotationNote::sign(&author.id, NoteAudience::Group(group.clone()), &ep, 1).wire(),
        );
        let private_endpoint = format!("PRIVATE-{name}-1");
        w.live.insert(ep, did.clone());
        w.live.insert(private_endpoint.clone(), did.clone());
        w.private_endpoints_ever.insert(private_endpoint.clone());
        w.nodes.push(Node {
            name,
            author,
            did: did.clone(),
            view: GroupView {
                group: group.clone(),
                roster: roster.clone(),
                accept_only: (did != host_did).then(|| [did.clone(), host_did.clone()].into()),
                history: BTreeMap::new(),
                frozen: BTreeSet::new(),
            },
            group_seq: 1,
            private_seq: 1,
            private_endpoint,
            private_contacts: roster.iter().filter(|x| **x != did).cloned().collect(),
            blocked: BTreeSet::new(),
            online: true,
            relayed: vec![],
        });
    }
    // Join bootstrap (invite/ticket) is out of scope: the host's view starts with every seq-1
    // note, a member's with its own and the host's (U7).
    for node in &mut w.nodes {
        for b in &bootstrap {
            match node.view.accept(policy, b) {
                Ok(_) | Err(Reject::NotHost) => {}
                Err(e) => panic!("bootstrap note refused: {e:?}"),
            }
        }
    }
    w
}
impl World {
    pub(super) fn did(&self, i: usize) -> String {
        self.nodes[i].did.clone()
    }
    pub(super) fn host_snapshot(&self) -> Vec<u8> {
        let h = &self.nodes[HOST];
        serde_json::to_vec(&(&h.view, &h.relayed)).unwrap()
    }
    pub(super) fn view_snapshot(&self, i: usize) -> Vec<u8> {
        serde_json::to_vec(&self.nodes[i].view).unwrap()
    }
    pub(super) fn known_endpoint(&self, viewer: usize, target: usize) -> Option<String> {
        self.nodes[viewer]
            .view
            .current(&self.nodes[target].did)
            .map(|n| n.claims.endpoint.clone())
    }
    pub(super) fn reach(&self, from: usize, to: usize) -> Result<(), RouteErr> {
        if !self.nodes[from].online {
            return Err(RouteErr::SelfOffline);
        }
        let did = &self.nodes[to].did;
        let Some(note) = self.nodes[from].view.route(did) else {
            return Err(RouteErr::NoBinding);
        };
        if self.live.get(&note.claims.endpoint) != Some(did) || !self.nodes[to].online {
            return Err(RouteErr::EndpointGone);
        }
        Ok(())
    }
    pub(super) fn send(&mut self, from: usize, to: usize, bytes: &[u8]) {
        self.wire.push(Frame {
            from: self.did(from),
            to: self.did(to),
            bytes: bytes.to_vec(),
        });
    }
    // Raw adversarial delivery (no route check) straight into `to`'s group view.
    pub(super) fn inject(
        &mut self,
        from: usize,
        to: usize,
        bytes: &[u8],
    ) -> Result<RotationNote, Reject> {
        self.send(from, to, bytes);
        let policy = self.policy;
        self.nodes[to].view.accept(policy, bytes)
    }
    // Periodic group-endpoint rotation (never triggered by a block). Old endpoint stops
    // listening; new one is a fresh fixture string, not derived from anything.
    pub(super) fn rotate_group(&mut self, i: usize) -> Vec<u8> {
        let n = &mut self.nodes[i];
        let old = n.view.current(&n.did).unwrap().claims.endpoint.clone();
        n.group_seq += 1;
        let ep = format!("grp-{}-{}", n.name, n.group_seq);
        let note = RotationNote::sign(
            &n.author.id,
            NoteAudience::Group(self.group.clone()),
            &ep,
            n.group_seq,
        );
        let bytes = note.wire();
        n.view.accept(self.policy, &bytes).unwrap();
        self.live.remove(&old);
        self.live.insert(ep, n.did.clone());
        bytes
    }
    // Private-audience rotation: sent directly to unblocked contacts, never via the host.
    pub(super) fn rotate_private(&mut self, i: usize) -> Vec<String> {
        let n = &mut self.nodes[i];
        self.live.remove(&n.private_endpoint);
        n.private_seq += 1;
        n.private_endpoint = format!("PRIVATE-{}-{}", n.name, n.private_seq);
        self.live.insert(n.private_endpoint.clone(), n.did.clone());
        self.private_endpoints_ever
            .insert(n.private_endpoint.clone());
        let note = RotationNote::sign(
            &n.author.id,
            NoteAudience::Private,
            &n.private_endpoint,
            n.private_seq,
        );
        let recipients: Vec<String> = n
            .private_contacts
            .iter()
            .filter(|c| !n.blocked.contains(*c))
            .cloned()
            .collect();
        for to in &recipients {
            self.private_wire.push(Frame {
                from: n.did.clone(),
                to: to.clone(),
                bytes: note.wire(),
            });
        }
        recipients
    }
    // Host relays its OWN note to every member it can reach. U7: never a member's note.
    pub(super) fn fanout(&mut self, bytes: &[u8]) -> Vec<usize> {
        let note: RotationNote = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            note.claims.owner,
            self.did(HOST),
            "U7: host relays only its own note"
        );
        let mut delivered = vec![];
        for j in 1..self.nodes.len() {
            if self.reach(HOST, j).is_err() {
                continue;
            }
            self.send(HOST, j, bytes);
            self.nodes[HOST].relayed.push(bytes.to_vec());
            let policy = self.policy;
            if self.nodes[j].view.accept(policy, bytes).is_ok() {
                delivered.push(j);
            }
        }
        delivered
    }
    // A member sends its own group note to the host only (U1 group audience, U7); the host
    // sends its own note to the members. Returns the members that accepted a host note.
    pub(super) fn announce(&mut self, i: usize, bytes: &[u8]) -> Result<Vec<usize>, RouteErr> {
        if i == HOST {
            return Ok(self.fanout(bytes));
        }
        self.reach(i, HOST)?;
        self.send(i, HOST, bytes);
        let policy = self.policy;
        let _ = self.nodes[HOST].view.accept(policy, bytes);
        Ok(vec![])
    }
    // U7 host retry: the host dials a member at the member's current group endpoint (only the
    // host knows it) and hands over its own current note. Returns whether it was accepted.
    pub(super) fn host_push(&mut self, j: usize) -> Result<bool, RouteErr> {
        self.reach(HOST, j)?;
        let bytes = self.nodes[HOST]
            .view
            .current(&self.did(HOST))
            .unwrap()
            .wire();
        self.send(HOST, j, &bytes);
        self.nodes[HOST].relayed.push(bytes.clone());
        let policy = self.policy;
        Ok(self.nodes[j].view.accept(policy, &bytes).is_ok())
    }
    // Member returns: its live side retries its own notes to the host (latest only or full
    // history, G-HISTORY) and pulls the host's own notes. Returns (accepted by the host,
    // accepted by the member).
    pub(super) fn catch_up(&mut self, i: usize) -> Result<(usize, usize), RouteErr> {
        self.reach(i, HOST)?;
        let mode = self.policy.catch_up;
        let own = self.nodes[i].view.export_owner(mode, &self.did(i));
        let mut to_host = 0;
        for bytes in own {
            self.send(i, HOST, &bytes);
            let policy = self.policy;
            if self.nodes[HOST].view.accept(policy, &bytes).is_ok() {
                to_host += 1;
            }
        }
        let notes = self.nodes[HOST].view.export_owner(mode, &self.did(HOST));
        let mut to_member = 0;
        for bytes in notes {
            self.send(HOST, i, &bytes);
            self.nodes[HOST].relayed.push(bytes.clone());
            let policy = self.policy;
            if self.nodes[i].view.accept(policy, &bytes).is_ok() {
                to_member += 1;
            }
        }
        Ok((to_host, to_member))
    }
    pub(super) fn group_bytes_scan(&self) -> Vec<Vec<u8>> {
        let mut hay = vec![self.host_snapshot()];
        hay.extend(self.wire.iter().map(|f| f.bytes.clone()));
        hay.extend(
            self.nodes
                .iter()
                .map(|n| serde_json::to_vec(&n.view).unwrap()),
        );
        hay
    }
}
pub(super) fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}
