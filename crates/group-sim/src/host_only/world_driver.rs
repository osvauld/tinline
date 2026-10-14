use super::*;

impl World {
    pub(super) fn chat_inbox(&mut self, j: usize, bytes: &[u8]) -> ChatIn {
        let Ok(w) = serde_json::from_slice::<Sealed>(bytes) else {
            return ChatIn::Dropped;
        };
        let me = self.did(j);
        let Some(s) = open(&StaticSecret::from(self.d[j].req_secret), &w) else {
            return ChatIn::Dropped;
        };
        let (group, from, to) = s.inner.parties();
        if !s.verify() || group != GROUP || to != me || from != w.header.from {
            return ChatIn::Dropped;
        }
        let digest = s.digest();
        // Blocking is NOT modelled on the chat path. U5 revised (visible blocking) leaves the
        // notice shape open (decisions section 8b); W2-Q owns that path.
        if self.d[j].seen.contains(&digest) {
            return ChatIn::Dropped;
        }
        match &s.inner {
            Inner::Request {
                from, request_id, ..
            } => {
                if self.d[j].incoming.contains_key(request_id) {
                    return ChatIn::Dropped;
                }
                let dv = &mut self.d[j];
                dv.seen.insert(digest.clone());
                dv.incoming
                    .insert(request_id.clone(), (from.clone(), digest));
                ChatIn::Request {
                    from: from.clone(),
                    request_id: request_id.clone(),
                }
            }
            Inner::Accept {
                from,
                request_id,
                request_digest,
                endpoint,
                ..
            } => {
                let ok = self.d[j].outgoing.get(request_id)
                    == Some(&(from.clone(), request_digest.clone()));
                if !ok {
                    return ChatIn::Dropped;
                }
                let dv = &mut self.d[j];
                dv.seen.insert(digest);
                dv.private_book.insert(from.clone(), endpoint.clone());
                ChatIn::Accepted { from: from.clone() }
            }
        }
    }
    pub(super) fn chat_accept(&mut self, j: usize, request_id: &str) -> Vec<(usize, ChatIn)> {
        let (from, digest) = self.d[j].incoming[request_id].clone();
        let claim = self.d[j].req_keys[&from].clone();
        let hep = self.reach_host(j).unwrap();
        let inner = Inner::Accept {
            group: GROUP.into(),
            from: self.did(j),
            to: from.clone(),
            request_id: request_id.into(),
            request_digest: digest,
            endpoint: self.d[j].private_ep(),
        };
        let signed = SignedInner::new(&self.d[j].id, inner);
        let header = Header {
            group: GROUP.into(),
            kind: ChatKind::Answer,
            from: self.did(j),
            to: from,
            ephemeral: [0; 32],
        };
        let eph = self.rng.bytes32();
        let bytes = serde_json::to_vec(&seal(header, &claim.claim.x25519, eph, &signed)).unwrap();
        self.send(j, HOST, hep, "chat", bytes.clone());
        self.host_route_chat(&bytes)
    }
    // Direct private-endpoint link after acceptance; the host is not on this path.
    pub(super) fn direct(&mut self, i: usize, to: &str, text: &str) -> Result<(), RouteErr> {
        let to_ep = self.dial(i, to)?;
        let j = self.listening(&to_ep).unwrap();
        let hello = Hello {
            from: self.did(i),
            to: to.into(),
            endpoint: self.d[i].private_ep(),
            text: text.into(),
        };
        let sig = did_sign(&self.d[i].id, "hello", &hello);
        self.send(
            i,
            j,
            to_ep,
            "direct",
            serde_json::to_vec(&(&hello, &sig)).unwrap(),
        );
        assert!(did_verify(&hello.from, "hello", &hello, &sig));
        self.d[j].private_book.insert(hello.from, hello.endpoint);
        Ok(())
    }
    pub(super) fn assert_converged(&self, devices: &[usize]) {
        let h = &self.d[HOST].board;
        for &i in devices {
            let b = &self.d[i].board;
            assert!(b.pending.is_empty(), "device {i} has pending posts");
            assert_eq!(
                b.accepted.keys().collect::<Vec<_>>(),
                h.accepted.keys().collect::<Vec<_>>(),
                "device {i} accepted set differs from host"
            );
            assert_eq!(b.doc.get_deep_value(), h.doc.get_deep_value());
        }
    }
}
pub(super) fn world() -> World {
    World::new(Cfg::default())
}
pub(super) const ALL: [usize; 5] = [HOST, A1, A2, BOB, CAROL];
