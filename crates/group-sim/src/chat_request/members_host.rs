use super::*;

pub(super) struct Member {
    pub(super) id: Identity,
    pub(super) seed: u8,
    pub(super) state: MemberState,
}
impl Member {
    pub(super) fn new(seed: u8, name: &str, policy: Policy) -> Self {
        // Reuse the shared identity fixture (real DID + Ed25519).
        let Author { id, .. } = Author::new(seed, GROUP);
        let tag = |label: &str| hex(&sim_random(seed, label, 0)[..6]);
        let mut m = Member {
            id,
            seed,
            state: MemberState {
                rng: 0,
                private_endpoint: format!("private-ep-{name}-{}", tag("private-endpoint")),
                group_endpoint: format!("group-ep-{name}-{}", tag("group-endpoint")),
                current_seq: 0,
                keys: BTreeMap::new(),
                known_keys: BTreeMap::new(),
                outgoing: BTreeMap::new(),
                incoming: BTreeMap::new(),
                seen: BTreeSet::new(),
                contacts: BTreeMap::new(),
                blocked: BTreeSet::new(),
                request_times: BTreeMap::new(),
                direct_inbox: vec![],
                policy,
            },
        };
        m.add_key();
        m
    }
    pub(super) fn did(&self) -> String {
        self.id.did().to_string()
    }
    pub(super) fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&self.state).unwrap()
    }
    pub(super) fn next_random(&mut self, label: &str) -> [u8; 32] {
        self.state.rng += 1;
        sim_random(self.seed, label, self.state.rng)
    }
    pub(super) fn add_key(&mut self) {
        let secret = self.next_random("request-key");
        let public = PublicKey::from(&StaticSecret::from(secret)).to_bytes();
        self.state.current_seq += 1;
        self.state.keys.insert(
            self.state.current_seq,
            KeySlot {
                secret: Some(secret),
                public,
                retire_at: None,
            },
        );
    }
    pub(super) fn key_note(&self) -> SignedRequestKey {
        let claim = RequestKeyClaim {
            group: GROUP.into(),
            owner: self.did(),
            seq: self.state.current_seq,
            x25519: self.state.keys[&self.state.current_seq].public,
        };
        let sig = did_sign(&self.id, "request-key", &claim);
        SignedRequestKey { claim, sig }
    }
    pub(super) fn rotate_request_key(&mut self, now: u64) -> SignedRequestKey {
        let grace = self.state.policy.grace;
        for slot in self.state.keys.values_mut() {
            slot.retire_at.get_or_insert(now + grace);
        }
        self.add_key();
        self.key_note()
    }
    // Erases secrets whose retirement time has passed.
    pub(super) fn tick(&mut self, now: u64) {
        for slot in self.state.keys.values_mut() {
            if slot.retire_at.is_some_and(|r| now >= r) {
                slot.secret = None;
            }
        }
    }
    // Never returns a retired key, even if `tick` has not erased it yet.
    pub(super) fn usable_secret(&self, seq: u64, now: u64) -> Option<StaticSecret> {
        let slot = self.state.keys.get(&seq)?;
        if slot.retire_at.is_some_and(|r| now >= r) {
            return None;
        }
        slot.secret.map(StaticSecret::from)
    }
    pub(super) fn learn_key(&mut self, note: SignedRequestKey) -> bool {
        if !note.verify() || note.claim.group != GROUP || note.claim.owner == self.did() {
            return false;
        }
        if let Some(k) = self.state.known_keys.get(&note.claim.owner) {
            if k.claim.seq >= note.claim.seq {
                return false;
            }
        }
        self.state.known_keys.insert(note.claim.owner.clone(), note);
        true
    }
    pub(super) fn sync_keys(&mut self, host: &Host) {
        for note in host.state.directory.values() {
            self.learn_key(note.clone());
        }
    }
    pub(super) fn block(&mut self, did: &str) {
        self.state.blocked.insert(did.into());
    }
    pub(super) fn request(&mut self, to: &str, now: u64) -> Option<(String, Relay)> {
        let note = self.state.known_keys.get(to)?.clone();
        let request_id = hex(&self.next_random("request-id")[..16]);
        let signed = SignedInner::new(
            &self.id,
            Inner::Request {
                group: GROUP.into(),
                from: self.did(),
                to: to.into(),
                request_id: request_id.clone(),
                key_seq: note.claim.seq,
                endpoint: self.state.private_endpoint.clone(),
                sent_at: now,
            },
        );
        let header = Header {
            group: GROUP.into(),
            kind: WireKind::Request,
            from: self.did(),
            to: to.into(),
            key_seq: note.claim.seq,
            ephemeral: [0; 32],
        };
        let eph = self.next_random("ephemeral");
        let wire = seal(header, &note.claim.x25519, eph, &signed);
        self.state.outgoing.insert(
            request_id.clone(),
            Outgoing {
                to: to.into(),
                sent_at: now,
                key_seq: note.claim.seq,
                digest: signed.digest(),
                answer: None,
            },
        );
        Some((request_id, Relay::Sealed(wire)))
    }
    pub(super) fn answer(&mut self, request_id: &str, accept: bool) -> Option<Relay> {
        let inc = self.state.incoming.remove(request_id)?;
        if !accept && self.state.policy.decline == DeclinePolicy::Silent {
            return None;
        }
        let note = self.state.known_keys.get(&inc.from)?.clone();
        let inner = if accept {
            self.state
                .contacts
                .insert(inc.from.clone(), inc.endpoint.clone());
            Inner::Accept {
                group: GROUP.into(),
                from: self.did(),
                to: inc.from.clone(),
                request_id: request_id.into(),
                request_digest: inc.digest,
                endpoint: self.state.private_endpoint.clone(),
            }
        } else {
            Inner::Decline {
                group: GROUP.into(),
                from: self.did(),
                to: inc.from.clone(),
                request_id: request_id.into(),
                request_digest: inc.digest,
            }
        };
        let signed = SignedInner::new(&self.id, inner);
        let header = Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: self.did(),
            to: inc.from,
            key_seq: note.claim.seq,
            ephemeral: [0; 32],
        };
        let eph = self.next_random("ephemeral");
        Some(Relay::Sealed(seal(
            header,
            &note.claim.x25519,
            eph,
            &signed,
        )))
    }
    pub(super) fn accept(&mut self, request_id: &str) -> Option<Relay> {
        self.answer(request_id, true)
    }
    pub(super) fn decline(&mut self, request_id: &str) -> Option<Relay> {
        self.answer(request_id, false)
    }
    pub(super) fn observe(&self, request_id: &str, now: u64) -> Observation {
        let o = &self.state.outgoing[request_id];
        match &o.answer {
            Some(Answer::Accepted(ep)) => Observation::Accepted(ep.clone()),
            Some(Answer::Refused) => Observation::Refused,
            Some(Answer::Blocked) => Observation::Blocked,
            Some(Answer::KeyStale) => Observation::KeyStale,
            None if now >= o.sent_at + ANSWER_TIMEOUT => Observation::NoAnswer,
            None => Observation::Pending,
        }
    }
    // All drops return before any state mutation.
    pub(super) fn receive(&mut self, relay: Relay, now: u64) -> (Delivery, Vec<Relay>) {
        let dropped = |r| (Delivery::Dropped(r), vec![]);
        let me = self.did();
        let wire = match relay {
            Relay::KeyHint { from, to, note } => {
                if to != me || note.claim.owner != from {
                    return dropped(DropReason::Misaddressed);
                }
                let stale: Vec<String> = self
                    .state
                    .outgoing
                    .iter()
                    .filter(|(_, o)| {
                        o.to == from && o.answer.is_none() && o.key_seq < note.claim.seq
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                if stale.is_empty() {
                    return dropped(DropReason::UnknownRequest);
                }
                if !self.learn_key(note) {
                    return dropped(DropReason::BadKeyNote);
                }
                for rid in stale {
                    self.state.outgoing.get_mut(&rid).unwrap().answer = Some(Answer::KeyStale);
                }
                return (Delivery::KeyHint(from), vec![]);
            }
            Relay::Sealed(w) => w,
        };
        let h = &wire.header;
        if h.group != GROUP || h.to != me {
            return dropped(DropReason::Misaddressed);
        }
        let Some(secret) = self.usable_secret(h.key_seq, now) else {
            let mut replies = vec![];
            if h.kind == WireKind::Request
                && self.state.policy.retired_key == RetiredKeyPolicy::HintCurrentKey
                && !self.state.blocked.contains(&h.from)
            {
                replies.push(Relay::KeyHint {
                    from: me,
                    to: h.from.clone(),
                    note: self.key_note(),
                });
            }
            return (Delivery::Dropped(DropReason::StaleKey), replies);
        };
        let Some(signed) = open(&secret, &wire) else {
            return dropped(DropReason::Undecryptable);
        };
        if !signed.verify() {
            return dropped(DropReason::BadSignature);
        }
        let digest = signed.digest();
        match signed.inner {
            Inner::Request {
                group,
                from,
                to,
                request_id,
                key_seq,
                endpoint,
                ..
            } => {
                if h.kind != WireKind::Request
                    || group != GROUP
                    || to != me
                    || from != h.from
                    || key_seq != h.key_seq
                {
                    return dropped(DropReason::Misaddressed);
                }
                if self.state.seen.contains(&request_id) {
                    return dropped(DropReason::Replay);
                }
                // Blocked requester: no state change. Whether a reply goes back is the open
                // 8b Q4 candidate (`BlockReply`).
                if self.state.blocked.contains(&from) {
                    let replies = match self.state.policy.block {
                        BlockReply::Drop => vec![],
                        BlockReply::SealedNotice => self
                            .blocked_answer(&from, &request_id, &digest)
                            .into_iter()
                            .collect(),
                    };
                    return (Delivery::Dropped(DropReason::Blocked), replies);
                }
                if let Some(limit) = self.state.policy.target_rate {
                    let recent = self.state.request_times.get(&from).map_or(0, |t| {
                        t.iter().filter(|&&at| at + limit.window > now).count()
                    });
                    if recent >= limit.max {
                        return dropped(DropReason::RateLimited);
                    }
                }
                self.state.seen.insert(request_id.clone());
                self.state
                    .request_times
                    .entry(from.clone())
                    .or_default()
                    .push(now);
                self.state.incoming.insert(
                    request_id.clone(),
                    Incoming {
                        from,
                        endpoint,
                        digest,
                        received_at: now,
                    },
                );
                (Delivery::NewRequest(request_id), vec![])
            }
            Inner::Accept {
                ref group,
                ref from,
                ref to,
                ref request_id,
                ref request_digest,
                ..
            }
            | Inner::Decline {
                ref group,
                ref from,
                ref to,
                ref request_id,
                ref request_digest,
            }
            | Inner::Blocked {
                ref group,
                ref from,
                ref to,
                ref request_id,
                ref request_digest,
            } => {
                if h.kind != WireKind::Answer || group != GROUP || *to != me || *from != h.from {
                    return dropped(DropReason::Misaddressed);
                }
                let Some(o) = self.state.outgoing.get(request_id) else {
                    return dropped(DropReason::UnknownRequest);
                };
                if o.to != *from || o.digest != *request_digest {
                    return dropped(DropReason::Misaddressed);
                }
                if o.answer.is_some() {
                    return dropped(DropReason::Replay);
                }
                let rid = request_id.clone();
                let answer = match &signed.inner {
                    Inner::Accept { endpoint, from, .. } => {
                        self.state.contacts.insert(from.clone(), endpoint.clone());
                        Answer::Accepted(endpoint.clone())
                    }
                    Inner::Blocked { .. } => Answer::Blocked,
                    _ => Answer::Refused,
                };
                self.state.outgoing.get_mut(&rid).unwrap().answer = Some(answer);
                (Delivery::Answered(rid), vec![])
            }
        }
    }
    // Candidate 8b Q4: the sealed "blocked" answer. Read-only: the ephemeral key is derived
    // from the request ID instead of the member RNG, so refusing leaves the state unchanged.
    pub(super) fn blocked_answer(&self, to: &str, request_id: &str, digest: &str) -> Option<Relay> {
        let note = self.state.known_keys.get(to)?;
        let signed = SignedInner::new(
            &self.id,
            Inner::Blocked {
                group: GROUP.into(),
                from: self.did(),
                to: to.into(),
                request_id: request_id.into(),
                request_digest: digest.into(),
            },
        );
        let header = Header {
            group: GROUP.into(),
            kind: WireKind::Answer,
            from: self.did(),
            to: to.into(),
            key_seq: note.claim.seq,
            ephemeral: [0; 32],
        };
        let counter = u64::from_le_bytes(
            blake3::hash(request_id.as_bytes()).as_bytes()[..8]
                .try_into()
                .unwrap(),
        );
        let eph = sim_random(self.seed, "blocked-ephemeral", counter);
        Some(Relay::Sealed(seal(
            header,
            &note.claim.x25519,
            eph,
            &signed,
        )))
    }
    // Test-only insider view: decrypt a wire with this member's key (a recipient can always
    // do this; used to model a malicious recipient re-encrypting what it received).
    pub(super) fn insider_open(&self, wire: &Wire, now: u64) -> Option<SignedInner> {
        open(&self.usable_secret(wire.header.key_seq, now)?, wire)
    }
    pub(super) fn direct_send(&self, to: &str, text: &str) -> Option<DirectMsg> {
        let body = DirectBody {
            to_endpoint: self.state.contacts.get(to)?.clone(),
            from: self.did(),
            text: text.into(),
        };
        let sig = did_sign(&self.id, "direct", &body);
        Some(DirectMsg { body, sig })
    }
    pub(super) fn direct_receive(&mut self, m: &DirectMsg) -> bool {
        let b = &m.body;
        if b.to_endpoint != self.state.private_endpoint
            || !self.state.contacts.contains_key(&b.from)
            || self.state.blocked.contains(&b.from)
            || !did_verify(&b.from, "direct", b, &m.sig)
        {
            return false;
        }
        self.state
            .direct_inbox
            .push((b.from.clone(), b.text.clone()));
        true
    }
}
// Direct network: delivers by private endpoint only; the host is not a parameter.
pub(super) fn route_direct(msg: &DirectMsg, members: &mut [&mut Member]) -> bool {
    members
        .iter_mut()
        .find(|m| m.state.private_endpoint == msg.body.to_endpoint)
        .is_some_and(|m| m.direct_receive(msg))
}

// ---------------------------------------------------------------- host

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HostRefusal {
    NotMember,
    RateLimited,
}
#[derive(Clone, Default, Serialize)]
pub(super) struct HostState {
    // U2: the host legitimately knows group endpoints (positive control for leak scans).
    pub(super) group_endpoints: BTreeMap<String, String>,
    pub(super) directory: BTreeMap<String, SignedRequestKey>,
    pub(super) mailboxes: BTreeMap<String, Vec<Relay>>,
    // key = "from\nto", accepted request times.
    pub(super) rate: BTreeMap<String, Vec<u64>>,
    pub(super) relayed: Vec<Relay>,
}
#[derive(Default)]
pub(super) struct Host {
    pub(super) state: HostState,
    pub(super) rate: Option<RateLimit>,
}
impl Host {
    pub(super) fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&self.state).unwrap()
    }
    pub(super) fn join(&mut self, m: &Member) {
        self.state
            .group_endpoints
            .insert(m.did(), m.state.group_endpoint.clone());
        assert!(self.publish(m.key_note()));
    }
    pub(super) fn publish(&mut self, note: SignedRequestKey) -> bool {
        if !note.verify()
            || note.claim.group != GROUP
            || !self.state.group_endpoints.contains_key(&note.claim.owner)
            || self
                .state
                .directory
                .get(&note.claim.owner)
                .is_some_and(|k| k.claim.seq >= note.claim.seq)
        {
            return false;
        }
        self.state.directory.insert(note.claim.owner.clone(), note);
        true
    }
    pub(super) fn submit(&mut self, relay: Relay, now: u64) -> Result<(), HostRefusal> {
        let (from, to) = relay.route();
        let (from, to) = (from.to_string(), to.to_string());
        if !self.state.group_endpoints.contains_key(&from)
            || !self.state.group_endpoints.contains_key(&to)
        {
            return Err(HostRefusal::NotMember);
        }
        let is_request = matches!(&relay, Relay::Sealed(w) if w.header.kind == WireKind::Request);
        let key = format!("{from}\n{to}");
        if let (true, Some(limit)) = (is_request, self.rate) {
            let recent = self.state.rate.get(&key).map_or(0, |t| {
                t.iter().filter(|&&at| at + limit.window > now).count()
            });
            if recent >= limit.max {
                return Err(HostRefusal::RateLimited);
            }
        }
        if is_request {
            self.state.rate.entry(key).or_default().push(now);
        }
        self.state.relayed.push(relay.clone());
        self.state.mailboxes.entry(to).or_default().push(relay);
        Ok(())
    }
    pub(super) fn fetch(&mut self, did: &str) -> Vec<Relay> {
        self.state.mailboxes.remove(did).unwrap_or_default()
    }
    // Leak scan: serialized state AND the raw bytes of every stored/relayed ciphertext.
    pub(super) fn leaks(&self, needle: &str) -> bool {
        let n = needle.as_bytes();
        contains(&self.snapshot(), n)
            || self
                .state
                .relayed
                .iter()
                .chain(self.state.mailboxes.values().flatten())
                .any(|r| match r {
                    Relay::Sealed(w) => contains(&w.ciphertext, n),
                    Relay::KeyHint { .. } => false,
                })
    }
}

// ---------------------------------------------------------------- fixtures

pub(super) struct World {
    pub(super) host: Host,
    pub(super) alice: Member,
    pub(super) bob: Member,
    pub(super) carol: Member,
}
pub(super) fn world_with(alice: Policy, bob: Policy, carol: Policy) -> World {
    let mut w = World {
        host: Host::default(),
        alice: Member::new(31, "alice", alice),
        bob: Member::new(32, "bob", bob),
        carol: Member::new(33, "carol", carol),
    };
    for m in [&w.alice, &w.bob, &w.carol] {
        w.host.join(m);
    }
    for m in [&mut w.alice, &mut w.bob, &mut w.carol] {
        m.sync_keys(&w.host);
    }
    w
}
pub(super) fn world(p: Policy) -> World {
    world_with(p, p, p)
}
