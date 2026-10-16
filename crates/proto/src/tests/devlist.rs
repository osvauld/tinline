//! Signed device lists.

use super::*;

// ---- multi-device ------------------------------------------------------------------

fn list_of(a: &Peer, n: usize, seq: u64) -> SignedDeviceList {
    let devs: Vec<_> = (0..n).map(|_| (dev(), None)).collect();
    sign_device_list(&a.id, &devs, seq)
}

#[test]
fn device_list_roundtrip_and_bad_relay_dropped() {
    let a = peer();
    let (d1, d2) = (dev(), dev());
    let blob = sign_device_list(
        &a.id,
        &[
            (d1, Some("https://relay.example/".into())),
            (d2, Some("http://bad".into())),
        ],
        42,
    );
    let l = verify_device_list(&blob, a.id.did()).unwrap();
    assert_eq!((l.seq, l.did.as_str()), (42, a.id.did()));
    assert_eq!(l.devices[0].device, d1);
    assert_eq!(
        l.devices[0].relay.as_deref(),
        Some("https://relay.example/")
    );
    assert_eq!(l.devices[1].relay, None);
}

#[test]
fn device_list_forged_wrong_did_and_limits() {
    let (a, b) = (peer(), peer());
    let blob = list_of(&a, 2, 1);
    assert_eq!(
        verify_device_list(&blob, b.id.did()),
        Err(Error::WrongIssuer)
    );
    // Signature by someone else over a payload claiming A.
    let forged = Signed::from_parts(blob.body().to_vec(), *list_of(&b, 2, 1).sig());
    assert_eq!(
        verify_device_list(&forged, a.id.did()),
        Err(Error::BadSignature)
    );
    // Another domain's envelope (an attestation's bytes) is not a device list.
    let att = Signed::<DeviceList>::from_bytes(&attest(&a.id, a.dev, T0).to_bytes()).unwrap();
    assert!(verify_device_list(&att, a.id.did()).is_err());
    assert!(verify_device_list(&list_of(&a, MAX_LIST_DEVICES, 1), a.id.did()).is_ok());
    // Shape runs on open too: build raw envelopes that bypass the sign-time check.
    let raw = |l: &DeviceList| {
        let body = cryptography::codec::to_bytes(l).unwrap();
        let sig =
            a.id.sign(&cryptography::sign::input(DeviceList::DOMAIN, &[&body]));
        Signed::<DeviceList>::from_parts(body, sig)
    };
    let entry = |d| DeviceEntry {
        device: d,
        relay: None,
    };
    let mut l = DeviceList {
        did: a.id.did().into(),
        seq: 1,
        devices: vec![],
        links: vec![],
    };
    l.devices = (0..=MAX_LIST_DEVICES).map(|_| entry(dev())).collect();
    assert_eq!(
        verify_device_list(&raw(&l), a.id.did()),
        Err(Error::TooManyDevices)
    );
    assert_eq!(Signed::sign(&l, &a.id).err(), Some(Error::TooManyDevices));
    let d = dev();
    l.devices = vec![entry(d), entry(d)];
    assert_eq!(verify_device_list(&raw(&l), a.id.did()), Err(Error::Decode));
    assert_eq!(Signed::sign(&l, &a.id).err(), Some(Error::Decode));
    l.devices = vec![];
    assert!(verify_device_list(&raw(&l), a.id.did()).is_err());
}

#[test]
fn device_list_ordering() {
    let a = peer();
    let (old, new) = (list_of(&a, 1, 10), list_of(&a, 2, 11));
    assert!(newer(&new, &old) && !newer(&old, &new));
    assert!(!newer(&old, &old));
    // Tie: exactly one side wins, and it is the larger signature.
    let (x, y) = (list_of(&a, 1, 20), list_of(&a, 2, 20));
    assert_ne!(newer(&x, &y), newer(&y, &x));
    assert_eq!(newer(&x, &y), x.sig() > y.sig());
    let junk = Signed::<DeviceList>::from_parts(vec![0xff; 3], [0; 64]);
    assert!(newer(&old, &junk) && !newer(&junk, &old));
}

// ---- succession (private endpoint rotation) ----------------------------------------

#[test]
fn succession_chain_leads_back_to_a_known_key_only_with_old_key_signatures() {
    let (a, b) = (peer(), peer());
    let (k0, k1, k2) = (new_device_secret(), new_device_secret(), new_device_secret());
    let pk = |k: &[u8; 32]| cryptography::signature::public_key(k);
    let did = a.id.did();
    let l01 = sign_succession(&k0, did, pk(&k1));
    let l12 = sign_succession(&k1, did, pk(&k2));
    assert!(verify_succession(&l01, did));
    assert!(!verify_succession(&l01, b.id.did()), "bound to the DID");
    let blob = sign_device_list_with(&a.id, &[(pk(&k2), None)], &[l01.clone(), l12.clone()], 5);
    let l = verify_device_list(&blob, did).unwrap();
    let knows = |k: [u8; 32]| move |d: &[u8; 32]| *d == k;
    // Two rotations while we were away: the chain still leads back.
    assert!(succeeds(&l, &pk(&k2), knows(pk(&k0))));
    assert!(succeeds(&l, &pk(&k2), knows(pk(&k1))));
    // A key we never knew, or a link signed by the new key instead of the old one, is not one.
    assert!(!succeeds(&l, &pk(&k2), knows(dev())));
    let mut forged = l12.clone();
    forged.sig = sign_succession(&k2, did, pk(&k2)).sig;
    let blob = sign_device_list_with(&a.id, &[(pk(&k2), None)], &[l01, forged], 6);
    let l = verify_device_list(&blob, did).unwrap();
    assert!(!succeeds(&l, &pk(&k2), knows(pk(&k0))));
    // Too many links, or a malformed one, do not decode.
    let many: Vec<_> = (0..=MAX_LIST_LINKS).map(|_| sign_succession(&new_device_secret(), did, dev())).collect();
    let list = DeviceList { did: did.into(), seq: 1, devices: vec![DeviceEntry { device: dev(), relay: None }], links: many };
    assert_eq!(Signed::sign(&list, &a.id).err(), Some(Error::Decode));
}
