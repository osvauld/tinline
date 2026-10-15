//! Signed device lists.

use super::*;

// ---- multi-device ------------------------------------------------------------------

fn list_of(a: &Peer, n: usize, seq: u64) -> SignedBlob {
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
    assert_eq!(l.devices[0].device_key().unwrap(), d1);
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
    let forged = SignedBlob {
        signature: list_of(&b, 2, 1).signature,
        ..blob.clone()
    };
    assert_eq!(
        verify_device_list(&forged, a.id.did()),
        Err(Error::BadSignature)
    );
    // Another domain's blob (an attestation) is not a device list.
    assert!(verify_device_list(&attest(&a.id, a.dev, T0), a.id.did()).is_err());
    assert!(verify_device_list(&list_of(&a, MAX_LIST_DEVICES, 1), a.id.did()).is_ok());
    assert_eq!(
        verify_device_list(&list_of(&a, MAX_LIST_DEVICES + 1, 1), a.id.did()),
        Err(Error::TooManyDevices)
    );
    let d = dev();
    let dup = sign_device_list(&a.id, &[(d, None), (d, None)], 1);
    assert_eq!(verify_device_list(&dup, a.id.did()), Err(Error::Decode));
    let empty = sign_device_list(&a.id, &[], 1);
    assert!(verify_device_list(&empty, a.id.did()).is_err());
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
    let (xs, ys) = (dec(&x.signature).unwrap(), dec(&y.signature).unwrap());
    assert_eq!(newer(&x, &y), xs > ys);
    let junk = SignedBlob {
        payload: "!".into(),
        signature: "!".into(),
    };
    assert!(newer(&old, &junk) && !newer(&junk, &old));
}
