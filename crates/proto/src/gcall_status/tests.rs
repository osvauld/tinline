use super::super::gcall::{self, MAX_TEXT};
use super::*;

const NOW: u64 = 1_800_000_000_000;

fn ident(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

fn gid() -> String {
    "f0".repeat(32)
}

fn status(id: &Identity) -> CallHostStatus {
    CallHostStatus {
        v: GCALL_VERSION,
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
    let blob = sign_status(&h, s.clone());
    assert_eq!(open_status_for(&blob, &gid(), NOW).unwrap(), s);
    // The largest legal status (64-char gid and digest, extreme values) is well inside the bound.
    let json = serde_json::to_vec(&StatusClaim::HostStatus(s)).unwrap();
    assert!(json.len() < MAX_STATUS / 2, "{} B", json.len());
    for p in [
        PowerState::Mains,
        PowerState::Charging(0),
        PowerState::Battery(42),
    ] {
        let s = CallHostStatus {
            power: p,
            ..status(&h)
        };
        assert_eq!(open_status(&sign_status(&h, s.clone())).unwrap(), s);
    }
}

#[test]
fn gcall_status_forged_tampered_and_cross_kind_are_refused() {
    let h = ident(1);
    let m = ident(2);
    // Signed by someone other than the claimed holder.
    let forged = sign_blob(&m, GCALL_DOMAIN, &StatusClaim::HostStatus(status(&h)));
    assert_eq!(open_status(&forged), Err(Error::BadSignature));
    // Tampered after signing (a lie about busy).
    let good = sign_status(&h, status(&h));
    let mut s = status(&h);
    s.busy = false;
    let payload = enc(serde_json::to_vec(&StatusClaim::HostStatus(s)).unwrap());
    let tampered = SignedBlob {
        payload,
        signature: good.signature.clone(),
    };
    assert_eq!(open_status(&tampered), Err(Error::BadSignature));
    // Another domain.
    let other = sign_blob(
        &h,
        b"osvauld/p2p/grant/v1\0",
        &StatusClaim::HostStatus(status(&h)),
    );
    assert_eq!(open_status(&other), Err(Error::BadSignature));
    // Same domain, other kinds: a Leave is not a status, a status is not a Leave or Start.
    let cid = gcall::new_call_id();
    let leave = gcall::sign_leave(&h, &gid(), &cid, 1, NOW);
    assert_eq!(open_status(&leave), Err(Error::Decode));
    assert!(gcall::open_leave(&good).is_err());
    assert!(gcall::open_start(&good).is_err());
}

#[test]
fn gcall_status_for_another_group_or_expired_is_refused() {
    let h = ident(1);
    let blob = sign_status(&h, status(&h));
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
    let short = sign_status(
        &h,
        CallHostStatus {
            ttl_ms: 1_000,
            ..status(&h)
        },
    );
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
    assert_eq!(
        open_status(&sign_status(&h, big)),
        Err(Error::FrameTooLarge)
    );
    let junk = SignedBlob {
        payload: "A".repeat(MAX_STATUS * 2),
        signature: String::new(),
    };
    assert_eq!(open_status(&junk), Err(Error::FrameTooLarge));
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
        assert_eq!(
            open_status(&sign_status(&h, s.clone())),
            Err(Error::Decode),
            "{s:?}"
        );
    }
    let mut s = status(&h);
    s.host_ref.permit_digest = String::new();
    assert_eq!(open_status(&sign_status(&h, s)), Err(Error::Decode));
    let s = CallHostStatus {
        v: GCALL_VERSION + 1,
        ..status(&h)
    };
    assert_eq!(open_status(&sign_status(&h, s)), Err(Error::UnknownVersion));
}
