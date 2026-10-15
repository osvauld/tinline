use super::super::gcall::{self, MAX_TEXT};
use super::*;

const NOW: u64 = 1_800_000_000_000;

/// An envelope signed without the sign-time shape check: what a hostile peer can send.
fn raw<T: Signable>(v: &T, id: &Identity) -> Signed<T> {
    let body = cryptography::codec::to_bytes(v).unwrap();
    let sig = id.sign(&cryptography::sign::input(T::DOMAIN, &[&body]));
    Signed::from_parts(body, sig)
}

fn ident(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

fn gid() -> String {
    "f0".repeat(32)
}

fn status(id: &Identity) -> CallHostStatus {
    CallHostStatus {
        gid: gid(),
        host_ref: HostRef {
            holder_did: id.did().into(),
            device_slot: u64::MAX,
            permit_digest: "d".repeat(64),
        },
        class: DeviceClass::Phone,
        power: PowerState::Battery(100),
        network: NetworkKind::Metered,
        thermal: ThermalLevel::Critical,
        busy: true,
        load: 100,
        max_participants: MAX_PARTICIPANTS as u8,
        issued_ms: NOW,
        ttl_ms: STATUS_TTL_MS,
    }
}

#[test]
fn gcall_status_round_trips_and_stays_small() {
    let h = ident(1);
    let s = status(&h);
    let blob = Signed::sign(&s, &h).unwrap();
    assert_eq!(open_status_for(&blob, &gid(), NOW).unwrap(), s);
    // The largest legal status (64-char gid and digest, extreme values) is well inside the bound.
    assert!(
        blob.body().len() < MAX_STATUS / 2,
        "{} B",
        blob.body().len()
    );
    for p in [
        PowerState::Mains,
        PowerState::Charging(0),
        PowerState::Battery(42),
    ] {
        let s = CallHostStatus {
            power: p,
            ..status(&h)
        };
        assert_eq!(Signed::sign(&s, &h).unwrap().open().unwrap(), s);
    }
}

#[test]
fn gcall_status_forged_tampered_and_cross_kind_are_refused() {
    let h = ident(1);
    let m = ident(2);
    // Signed by someone other than the claimed holder.
    assert_eq!(
        Signed::sign(&status(&h), &m).err(),
        Some(Error::WrongIssuer)
    );
    assert_eq!(raw(&status(&h), &m).open(), Err(Error::BadSignature));
    // Tampered after signing (a lie about busy).
    let good = Signed::sign(&status(&h), &h).unwrap();
    let mut s = status(&h);
    s.busy = false;
    let tampered = Signed::<CallHostStatus>::from_parts(
        cryptography::codec::to_bytes(&s).unwrap(),
        *good.sig(),
    );
    assert_eq!(tampered.open(), Err(Error::BadSignature));
    // Another domain.
    let body = good.body().to_vec();
    let other = Signed::<CallHostStatus>::from_parts(
        body.clone(),
        h.sign(&cryptography::sign::input(
            cryptography::domains::GRANT,
            &[&body],
        )),
    );
    assert_eq!(other.open(), Err(Error::BadSignature));
    // Other kinds: a Leave is not a status, a status is not a Leave or Start (own domains).
    let cid = gcall::new_call_id();
    let leave = gcall::sign_leave(&h, &gid(), &cid, 1, NOW).unwrap();
    assert!(
        Signed::<CallHostStatus>::from_bytes(&leave.to_bytes())
            .unwrap()
            .open()
            .is_err()
    );
    assert!(
        Signed::<gcall::Leave>::from_bytes(&good.to_bytes())
            .unwrap()
            .open()
            .is_err()
    );
    assert!(
        Signed::<gcall::CallStart>::from_bytes(&good.to_bytes())
            .unwrap()
            .open()
            .is_err()
    );
}

#[test]
fn gcall_status_for_another_group_or_expired_is_refused() {
    let h = ident(1);
    let blob = Signed::sign(&status(&h), &h).unwrap();
    assert_eq!(
        open_status_for(&blob, "other-group", NOW),
        Err(Error::UnexpectedMessage)
    );
    // Alive for exactly ttl_ms.
    assert!(open_status_for(&blob, &gid(), NOW + STATUS_TTL_MS - 1).is_ok());
    assert_eq!(
        open_status_for(&blob, &gid(), NOW + STATUS_TTL_MS),
        Err(Error::Expired)
    );
    // A short TTL ends earlier.
    let short = Signed::sign(
        &CallHostStatus {
            ttl_ms: 1_000,
            ..status(&h)
        },
        &h,
    )
    .unwrap();
    assert_eq!(
        open_status_for(&short, &gid(), NOW + 1_000),
        Err(Error::Expired)
    );
    // Future-dated within the skew allowance counts; beyond it does not.
    assert!(open_status_for(&blob, &gid(), NOW - MAX_CLOCK_SKEW_MS).is_ok());
    assert_eq!(
        open_status_for(&blob, &gid(), NOW - MAX_CLOCK_SKEW_MS - 1),
        Err(Error::Expired)
    );
}

#[test]
fn gcall_status_oversize_and_bad_shape_are_refused() {
    let h = ident(1);
    // Oversize: refused before decoding or verifying.
    let big = CallHostStatus {
        gid: "g".repeat(2 * MAX_STATUS),
        ..status(&h)
    };
    assert_eq!(Signed::sign(&big, &h).err(), Some(Error::Decode));
    assert_eq!(raw(&big, &h).open().err(), Some(Error::TooLarge));
    let junk = vec![0u8; MAX_STATUS + signed::SIG_LEN + 1];
    assert_eq!(
        Signed::<CallHostStatus>::from_bytes(&junk).err(),
        Some(Error::TooLarge)
    );
    // Shape bounds.
    let bad: Vec<CallHostStatus> = vec![
        CallHostStatus {
            gid: "g".repeat(MAX_TEXT + 1),
            ..status(&h)
        },
        CallHostStatus {
            gid: String::new(),
            ..status(&h)
        },
        CallHostStatus {
            power: PowerState::Battery(101),
            ..status(&h)
        },
        CallHostStatus {
            power: PowerState::Charging(101),
            ..status(&h)
        },
        CallHostStatus {
            load: 101,
            ..status(&h)
        },
        CallHostStatus {
            max_participants: 1,
            ..status(&h)
        },
        CallHostStatus {
            max_participants: MAX_PARTICIPANTS as u8 + 1,
            ..status(&h)
        },
        CallHostStatus {
            ttl_ms: 0,
            ..status(&h)
        },
        CallHostStatus {
            ttl_ms: STATUS_TTL_MS + 1,
            ..status(&h)
        },
    ];
    for s in bad {
        assert_eq!(Signed::sign(&s, &h).err(), Some(Error::Decode), "{s:?}");
        assert_eq!(raw(&s, &h).open().err(), Some(Error::Decode), "{s:?}");
    }
    let mut s = status(&h);
    s.host_ref.permit_digest = String::new();
    assert_eq!(raw(&s, &h).open().err(), Some(Error::Decode));
}
