//! Test-only authority model: no expiry, revocation, wire compatibility or production API.
use identity::{Identity, public_key_from_did, verify};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const GENESIS_DOMAIN: &[u8] = b"tinline/group-sim/genesis/not-production/v1\0";
const GRANT_DOMAIN: &[u8] = b"tinline/group-sim/grant/not-production/v1\0";
pub(super) const MAX_CHAIN: usize = 8;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GenesisBody {
    pub version: u8,
    pub creator: String,
    pub nonce: [u8; 32],
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Genesis {
    pub body: GenesisBody,
    pub signature: Vec<u8>,
}
fn bytes<T: Serialize>(domain: &[u8], body: &T) -> Vec<u8> {
    [domain, &serde_json::to_vec(body).unwrap()].concat()
}
fn valid(did: &str, bytes: &[u8], sig: &[u8]) -> bool {
    let Some(key) = public_key_from_did(did) else {
        return false;
    };
    let Ok(sig) = <&[u8; 64]>::try_from(sig) else {
        return false;
    };
    verify(&key, bytes, sig)
}
impl Genesis {
    pub fn create(owner: &Identity, nonce: [u8; 32]) -> Self {
        let body = GenesisBody {
            version: 1,
            creator: owner.did().into(),
            nonce,
        };
        let signature = owner.sign(&bytes(GENESIS_DOMAIN, &body)).to_vec();
        Self { body, signature }
    }
    pub fn id(&self) -> String {
        blake3::hash(&bytes(GENESIS_DOMAIN, &self.body))
            .to_hex()
            .to_string()
    }
    pub fn verify(&self, expected_group: &str) -> bool {
        self.body.version == 1
            && self.id() == expected_group
            && valid(
                &self.body.creator,
                &bytes(GENESIS_DOMAIN, &self.body),
                &self.signature,
            )
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) enum Capability {
    Read,
    Post,
    Delegate,
    // DID-level capability fixture only: never proof of a nominated hosting device.
    Host,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GrantBody {
    pub version: u8,
    pub group: String,
    pub issuer: String,
    pub holder: String,
    pub capabilities: BTreeSet<Capability>,
    pub delegable: bool,
    pub nonce: u64,
    pub parent: Option<Box<Grant>>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Grant {
    pub body: GrantBody,
    pub signature: Vec<u8>,
}
impl Grant {
    pub fn issue(
        issuer: &Identity,
        holder: &str,
        group: &str,
        capabilities: &[Capability],
        delegable: bool,
        nonce: u64,
        parent: Option<Grant>,
    ) -> Self {
        let body = GrantBody {
            version: 1,
            group: group.into(),
            issuer: issuer.did().into(),
            holder: holder.into(),
            capabilities: capabilities.iter().copied().collect(),
            delegable,
            nonce,
            parent: parent.map(Box::new),
        };
        let signature = issuer.sign(&bytes(GRANT_DOMAIN, &body)).to_vec();
        Self { body, signature }
    }
    // Walk root-to-leaf only after bounded traversal. Presented holder must match signed author.
    pub fn allows(&self, genesis: &Genesis, holder: &str, capability: Capability) -> bool {
        let mut links = Vec::new();
        let mut current = Some(self);
        while let Some(g) = current {
            if links.len() >= MAX_CHAIN {
                return false;
            }
            links.push(g);
            current = g.body.parent.as_deref();
        }
        if self.body.holder != holder || !self.body.capabilities.contains(&capability) {
            return false;
        }
        let group = genesis.id();
        let mut parent: Option<&Grant> = None;
        for g in links.into_iter().rev() {
            let b = &g.body;
            if b.version != 1
                || b.group != group
                || public_key_from_did(&b.holder).is_none()
                || !valid(&b.issuer, &bytes(GRANT_DOMAIN, b), &g.signature)
            {
                return false;
            }
            match parent {
                None if b.issuer != genesis.body.creator => return false,
                Some(p)
                    if b.issuer != p.body.holder
                        || !p.body.delegable
                        || !p.body.capabilities.contains(&Capability::Delegate)
                        || !b.capabilities.is_subset(&p.body.capabilities) =>
                {
                    return false;
                }
                _ => {}
            }
            parent = Some(g);
        }
        true
    }
}

/// Posting authority only. Device succession, current session access and hosting
/// nomination are separate contracts, deliberately absent from this adapter.
pub(super) trait PostAuthorization {
    fn allows_post(&self, author: &str, permit: Option<&Grant>) -> bool;
}
pub(super) struct SignedGroupAuthority {
    genesis: Genesis,
}
impl SignedGroupAuthority {
    pub fn verified(genesis: Genesis, expected_group: &str) -> Option<Self> {
        genesis.verify(expected_group).then_some(Self { genesis })
    }
}
impl PostAuthorization for SignedGroupAuthority {
    fn allows_post(&self, author: &str, permit: Option<&Grant>) -> bool {
        permit.is_some_and(|p| p.allows(&self.genesis, author, Capability::Post))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity(seed: u8) -> Identity {
        identity::recover(
            &identity::Mnemonic::from_entropy(&[seed; 32])
                .unwrap()
                .to_string(),
        )
        .unwrap()
    }
    #[test]
    fn genesis_hash_signature_version_and_creator_are_checked() {
        let a = identity(1);
        let x = identity(3);
        let g = Genesis::create(&a, [9; 32]);
        assert!(g.verify(&g.id()));
        let mut altered = g.clone();
        altered.body.nonce[0] ^= 1;
        assert!(!altered.verify(&g.id()));
        assert!(!altered.verify(&altered.id()));
        let mut altered = g.clone();
        altered.body.creator = x.did().into();
        assert!(!altered.verify(&altered.id()));
        let mut altered = g.clone();
        altered.body.version = 2;
        altered.signature = a.sign(&bytes(GENESIS_DOMAIN, &altered.body)).to_vec();
        assert!(!altered.verify(&altered.id()));
    }
    #[test]
    fn delegation_is_rooted_scoped_narrowed_and_holder_bound() {
        let a = identity(1);
        let b = identity(2);
        let c = identity(3);
        let g = Genesis::create(&a, [9; 32]);
        let gid = g.id();
        let admin = Grant::issue(
            &a,
            b.did(),
            &gid,
            &[
                Capability::Read,
                Capability::Post,
                Capability::Delegate,
                Capability::Host,
            ],
            true,
            1,
            None,
        );
        let member = Grant::issue(
            &b,
            c.did(),
            &gid,
            &[Capability::Read, Capability::Post],
            false,
            2,
            Some(admin.clone()),
        );
        assert!(member.allows(&g, c.did(), Capability::Post));
        assert!(!member.allows(&g, b.did(), Capability::Post));
        assert!(!member.allows(&g, c.did(), Capability::Host));
        assert!(!member.allows(&Genesis::create(&a, [8; 32]), c.did(), Capability::Post));
        let mut bad = member.clone();
        bad.signature[0] ^= 1;
        assert!(!bad.allows(&g, c.did(), Capability::Post));
        let mut bad = member.clone();
        bad.body.parent.as_mut().unwrap().signature[0] ^= 1;
        assert!(!bad.allows(&g, c.did(), Capability::Post));
        let forged_root = Grant::issue(&b, c.did(), &gid, &[Capability::Post], false, 3, None);
        assert!(!forged_root.allows(&g, c.did(), Capability::Post));
    }
    #[test]
    fn nondelegable_grants_scope_widening_and_depth_are_refused() {
        let a = identity(1);
        let b = identity(2);
        let c = identity(3);
        let g = Genesis::create(&a, [9; 32]);
        let gid = g.id();
        for (caps, delegable) in [
            (vec![Capability::Read, Capability::Post], false),
            (vec![Capability::Read, Capability::Delegate], true),
            (vec![Capability::Read, Capability::Post], true),
        ] {
            let parent = Grant::issue(&a, b.did(), &gid, &caps, delegable, 1, None);
            let child = Grant::issue(
                &b,
                c.did(),
                &gid,
                &[Capability::Post],
                false,
                2,
                Some(parent),
            );
            assert!(!child.allows(&g, c.did(), Capability::Post));
        }
        let mut chain = Grant::issue(
            &a,
            b.did(),
            &gid,
            &[Capability::Post, Capability::Delegate],
            true,
            0,
            None,
        );
        for i in 1..MAX_CHAIN {
            chain = Grant::issue(
                &b,
                b.did(),
                &gid,
                &[Capability::Post, Capability::Delegate],
                true,
                i as u64,
                Some(chain),
            );
        }
        assert!(chain.allows(&g, b.did(), Capability::Post));
        chain = Grant::issue(
            &b,
            b.did(),
            &gid,
            &[Capability::Post],
            false,
            99,
            Some(chain),
        );
        assert!(!chain.allows(&g, b.did(), Capability::Post));
    }
}
