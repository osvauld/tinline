use super::*;

const P: RotationPolicy = RotationPolicy::ON;

fn me_with(secret: [u8; 32]) -> Me {
    let (id, phrase) = identity::generate();
    let _ = id;
    Me::load(Profile { mnemonic: phrase.to_string(), name: "synthetic".into(), device_secret: secret }, 1).unwrap()
}

fn entry(me: &Me, links: Vec<proto::Succession>) -> OwnDevice {
    OwnDevice { device: me.device, attestation: me.attestation.clone(), label: "phone".into(), removed: false, last_seen: 1, relay: None, links, rotated: false }
}

// KIND: unit (packet F) — completing a rotation tombstones the old key as `rotated` (not shown as
// unlinked), moves the install's links onto the new key with the new link last (at most
// `KEPT_LINKS`), restarts the schedule from the promote and opens the announce window.
#[test]
fn completing_a_rotation_moves_the_install_to_the_new_key() {
    let old_secret = proto::new_device_secret();
    let old = me_with(old_secret);
    let mut profile = old.profile.clone();
    profile.device_secret = proto::new_device_secret();
    let new = Me::load(profile, 1).unwrap();
    let earlier: Vec<_> = (0..KEPT_LINKS).map(|_| proto::sign_succession(&proto::new_device_secret(), old.id.did(), old.device)).collect();
    let mut st = State { registry: vec![entry(&old, earlier.clone())], ticket: Some("OSVC2:x".into()), ..State::default() };
    let p = PendingRotation { old: old.device, new: new.device, link: proto::sign_succession(&old_secret, old.id.did(), new.device), at: 5 };
    complete_rotation(&mut st, &new, &p, "phone".into(), 1_000, &P);
    let o = st.registry.iter().find(|e| e.device == old.device).unwrap();
    assert!(o.removed && o.rotated && o.links.is_empty());
    let n = st.registry.iter().find(|e| e.device == new.device).unwrap();
    assert!(!n.removed && !n.rotated);
    assert_eq!(n.links.len(), KEPT_LINKS);
    assert_eq!(n.links.last(), Some(&p.link));
    assert_eq!(&n.links[..KEPT_LINKS - 1], &earlier[1..]);
    assert_eq!((st.private_ep.made, st.private_ep.announce_until, st.private_ep.pending.clone()), (1_000, 1_000 + P.announce_ms, None));
    assert!(st.private_ep.jitter <= P.jitter_ms);
    assert_eq!(st.ticket, None, "the next ticket names the new key");
}

// KIND: unit (packet F) — a list signed after a rotation keeps a contact's `verified` (the link
// proves the old key named the new one); a new install of the same DID still resets it.
#[test]
fn verified_survives_a_rotated_key_but_not_a_new_install() {
    use crate::store::{ContactDevice, StoredContact};
    let old_secret = proto::new_device_secret();
    let them = me_with(old_secret);
    let new_secret = proto::new_device_secret();
    let new_key = cryptography::signature::public_key(&new_secret);
    let grant = proto::issue_token(&them.id, "did:key:zSynthetic", proto::Cap::Message, 1, 1000);
    let mut c = StoredContact {
        did: them.id.did().to_string(),
        name: "t".into(),
        devices: vec![ContactDevice { device: them.device, relay: None }],
        device_list: None,
        grant_from_them: grant,
        call_grant_from_them: None,
        calls_allowed: true,
        added_at: 1,
        alias: None,
        verified: true,
    };
    let link = proto::sign_succession(&old_secret, them.id.did(), new_key);
    let rotated = proto::sign_device_list_with(&them.id, &[(new_key, None)], &[link], now_ms());
    assert!(merge_device_list(&mut c, &rotated));
    assert!(c.verified, "a rotated key keeps it");
    assert_eq!(c.devices[0].device, new_key);
    let install = proto::sign_device_list(&them.id, &[(new_key, None), (proto::new_device_secret(), None)], now_ms() + 1);
    assert!(merge_device_list(&mut c, &install));
    assert!(!c.verified, "a new install resets it");
}
