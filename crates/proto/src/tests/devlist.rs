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
