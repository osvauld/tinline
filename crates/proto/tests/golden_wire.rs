//! Golden wire vectors for `proto`, frozen BEFORE the dependency upgrades (ed25519-dalek 3, sha2 0.11,
//! hkdf/hmac 0.13, aes-gcm 0.11, rand 0.10). Must pass UNCHANGED afterwards.
//!
//! Produced by `cargo test -p proto --test golden_wire -- --ignored --nocapture dump` on base 0e1eecb,
//! pasted into the tables below. Identities come from the public BIP-39 test phrases (synthetic).
//! Deterministic items (Ed25519 is deterministic, JSON field order is the struct order) are rebuilt and
//! compared byte for byte, then opened again. Items with random ids/nonces (tokens, tickets, joins, sealed
//! link grants) are checked on the decode/verify side from captured bytes.

use identity::{Identity, recover};
use proto::*;
use std::collections::HashSet;

const PHRASE_A: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PHRASE_B: &str =
    "legal winner thank year wave sausage worth useful legal winner thank yellow";

fn a() -> Identity {
    recover(PHRASE_A).unwrap()
}
fn b() -> Identity {
    recover(PHRASE_B).unwrap()
}
fn dev(n: u8) -> [u8; 32] {
    core::array::from_fn(|i| n.wrapping_add(i as u8))
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

const GID: &str = "grp-golden";
// 16 bytes 0x01.. in base64url.
const CALL_ID: &str = "AQIDBAUGBwgJCgsMDQ4PEA";
const T0: u64 = 1_800_000_000;

fn host(i: &Identity) -> gcall::HostRef {
    gcall::HostRef {
        holder_did: i.did().into(),
        device_slot: 3,
        permit_digest: "digest-golden".into(),
    }
}

/// Every deterministic signed blob, by name.
fn blobs() -> Vec<(&'static str, SignedBlob)> {
    let (a, b) = (a(), b());
    let start = gcall::CallStart {
        v: 1,
        gid: GID.into(),
        call_id: CALL_ID.into(),
        host_gen: 1,
        host_ref: host(&a),
        starter_did: a.did().into(),
        started_ms: T0 * 1000,
        k: 3,
        max_participants: 8,
        backups: vec![host(&b)],
        meta_frontier: "frontier-golden".into(),
    };
    let roster = gcall::Roster {
        v: 1,
        gid: GID.into(),
        call_id: CALL_ID.into(),
        host_gen: 1,
        host_did: a.did().into(),
        epoch: 2,
        entries: vec![gcall::RosterEntry {
            slot: 1,
            did: b.did().into(),
            issued_ms: T0 * 1000 + 5,
            nonce: 42,
            sig: "c2ln".into(),
        }],
    };
    let end = gcall::End {
        v: 1,
        gid: GID.into(),
        call_id: CALL_ID.into(),
        host_gen: 1,
        host_did: a.did().into(),
        reason: gcall::EndReason::HostRotating,
        next_host: Some(host(&b)),
    };
    let req = gcall::HostRequest {
        v: 1,
        gid: GID.into(),
        call_id: CALL_ID.into(),
        starter_did: a.did().into(),
        host: host(&b),
        backups: vec![],
        expected: 4,
        issued_ms: T0 * 1000,
    };
    let refusal = gcall::HostRefusal {
        v: 1,
        gid: GID.into(),
        call_id: CALL_ID.into(),
        starter_did: a.did().into(),
        host: host(&b),
        reason: "busy".into(),
    };
    let status = gcall_status::CallHostStatus {
        v: 1,
        gid: GID.into(),
        host_ref: host(&a),
        class: gcall_status::DeviceClass::Phone,
        power: gcall_status::PowerState::Battery(80),
        network: gcall_status::NetworkKind::Metered,
        thermal: gcall_status::ThermalLevel::Light,
        busy: false,
        load: 10,
        max_participants: 6,
        issued_ms: T0 * 1000,
        ttl_ms: 30_000,
    };
    vec![
        ("attestation", attest(&a, dev(1), T0)),
        (
            "device_list",
            sign_device_list(
                &a,
                &[
                    (dev(1), None),
                    (dev(50), Some("https://relay.example.test".into())),
                ],
                T0 * 1000,
            ),
        ),
        (
            "block_notice",
            sign_block_notice(&a, b.did(), true, T0 * 1000),
        ),
        (
            "unblock_notice",
            sign_block_notice(&a, b.did(), false, T0 * 1000 + 1),
        ),
        ("gcall_start", gcall::sign_start(&a, start)),
        ("gcall_roster", gcall::sign_roster(&a, roster)),
        (
            "gcall_leave",
            gcall::sign_leave(&b, GID, CALL_ID, 1, T0 * 1000 + 9),
        ),
        ("gcall_end", gcall::sign_end(&a, end)),
        ("gcall_host_request", gcall::sign_host_request(&a, req)),
        ("gcall_host_refusal", gcall::sign_host_refusal(&b, refusal)),
        (
            "gcall_note",
            gcall::sign_note(&b, &dev(7), &dev(9), T0 * 1000),
        ),
        ("gcall_status", gcall_status::sign_status(&a, status)),
        (
            "picture_deletion",
            profile::sign_picture_deletion(&a, &"ab".repeat(32)).unwrap(),
        ),
    ]
}

/// Encoded frames, by name.
fn frames() -> Vec<(&'static str, Vec<u8>)> {
    let (a, _) = (a(), b());
    let att = attest(&a, dev(1), T0);
    vec![
        ("ringing", encode_frame(&Msg::Ringing)),
        ("busy", encode_frame(&Msg::Busy)),
        ("hangup", encode_frame(&Msg::Hangup)),
        (
            "reject",
            encode_frame(&Msg::Reject {
                reason: CALLS_NOT_ALLOWED.into(),
            }),
        ),
        ("video", encode_frame(&Msg::Video { camera: true })),
        (
            "notice",
            encode_frame(&Msg::Notice {
                notice: sign_block_notice(&a, b().did(), true, T0 * 1000),
            }),
        ),
        (
            "devices",
            encode_frame(&Msg::Devices {
                list: sign_device_list(&a, &[(dev(1), None)], T0 * 1000),
            }),
        ),
        (
            "accept",
            encode_frame(&Msg::Accept {
                renewed_grant: None,
                devices: None,
                video: Some(VideoOffer {
                    max_pixels: 921_600,
                }),
            }),
        ),
        (
            "decline",
            encode_frame(&Msg::Decline {
                reason: "later".into(),
            }),
        ),
        (
            "cancel",
            encode_frame(&Msg::Cancel {
                reason: CancelReason::AnsweredElsewhere,
            }),
        ),
        (
            "tokens",
            encode_frame(&Msg::Tokens {
                grants: vec![att.clone()],
            }),
        ),
        (
            "group_letter",
            encode_frame(&Msg::GroupLetter {
                attestation: att,
                grant: attest(&a, dev(2), T0),
                letter: "bGV0dGVy".into(),
            }),
        ),
        (
            "gcall_heartbeat",
            gcall::encode_control(&gcall::GroupCallMsg::Heartbeat { epoch: 7 }).unwrap(),
        ),
        (
            "gcall_active",
            gcall::encode_control(&gcall::GroupCallMsg::Active {
                r#gen: 2,
                slots: vec![1, 4, 5],
            })
            .unwrap(),
        ),
    ]
}

fn link_qr(relay: Option<&str>) -> proto::link::LinkQr {
    // 1_767_225_600 is the wire epoch; expiry is minute resolution from it.
    proto::link::LinkQr {
        device: dev(3),
        secret: [9u8; 16],
        exp: 1_767_225_600 + 60 * 1000,
        relay: relay.map(Into::into),
    }
}

fn link_texts() -> Vec<(&'static str, String)> {
    vec![
        ("qr_none", link_qr(None).to_text()),
        (
            "qr_custom",
            link_qr(Some("https://relay.example.test")).to_text(),
        ),
    ]
}

fn link_proofs() -> Vec<(&'static str, String)> {
    use proto::link::{LinkRole, confirm_code, link_proof};
    let (s, e, n) = ([9u8; 16], dev(10), dev(20));
    vec![
        (
            "proof_scanner",
            hex(&link_proof(&s, &e, &n, LinkRole::Scanner, &e)),
        ),
        (
            "proof_displayer",
            hex(&link_proof(&s, &e, &n, LinkRole::Displayer, &n)),
        ),
        ("confirm_code", confirm_code(&s, &e, &n)),
    ]
}

// ---- captured tables ----
const BLOBS: &[(&str, &str, &str)] = &[
    (
        "attestation",
        "eyJ2IjoxLCJkaWQiOiJkaWQ6a2V5Ono2TWtubmNWbWVkcGhnRFNiUW9ZTHlOYUZSRmRCMld0eWp4Y0ZkS1Y2elRmY1l2cyIsImRldmljZSI6IkFRSURCQVVHQndnSkNnc01EUTRQRUJFU0V4UVZGaGNZR1JvYkhCMGVIeUEiLCJpYXQiOjE4MDAwMDAwMDB9",
        "svGZXAUiYib1GPonDcgNaKVD-oVIWh-xNCVV2ez70cgt60esPQUPH5MMH7RAE2w5KPYrXa4cE5CFoUt7uWfeCw",
    ),
    (
        "device_list",
        "eyJ2IjoxLCJkaWQiOiJkaWQ6a2V5Ono2TWtubmNWbWVkcGhnRFNiUW9ZTHlOYUZSRmRCMld0eWp4Y0ZkS1Y2elRmY1l2cyIsInNlcSI6MTgwMDAwMDAwMDAwMCwiZGV2aWNlcyI6W3siZGV2aWNlIjoiQVFJREJBVUdCd2dKQ2dzTURRNFBFQkVTRXhRVkZoY1lHUm9iSEIwZUh5QSIsInJlbGF5IjpudWxsfSx7ImRldmljZSI6Ik1qTTBOVFkzT0RrNk96dzlQajlBUVVKRFJFVkdSMGhKU2t0TVRVNVBVRkUiLCJyZWxheSI6Imh0dHBzOi8vcmVsYXkuZXhhbXBsZS50ZXN0In1dfQ",
        "zkYElW8lK0IcnXPOQmuvVM8hK8P5EPici0gp7-DfrQo4YcWps6T_F-rE5m7Lq9HDalyOPqYrXiV5sQcaYH7YAQ",
    ),
    (
        "block_notice",
        "eyJ2IjoxLCJmcm9tIjoiZGlkOmtleTp6Nk1rbm5jVm1lZHBoZ0RTYlFvWUx5TmFGUkZkQjJXdHlqeGNGZEtWNnpUZmNZdnMiLCJ0byI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiYmxvY2tlZCI6dHJ1ZSwiYXQiOjE4MDAwMDAwMDAwMDB9",
        "zbTRrsHhFTTbmBYDSHp9RIFH_msFKrb3PCx0TjxQ8XVPGO-Wui6g4jDZJARKICBed6LPVafh49y-w0UZBZQbCw",
    ),
    (
        "unblock_notice",
        "eyJ2IjoxLCJmcm9tIjoiZGlkOmtleTp6Nk1rbm5jVm1lZHBoZ0RTYlFvWUx5TmFGUkZkQjJXdHlqeGNGZEtWNnpUZmNZdnMiLCJ0byI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiYmxvY2tlZCI6ZmFsc2UsImF0IjoxODAwMDAwMDAwMDAxfQ",
        "OyZvqcydgRxKcwMD2B63JEEeUFse2m-1TMRl2WFoWR_NFmSyMiX1V8TwEDupslvXOBcgMDLFL1oMzsvfWc-NDw",
    ),
    (
        "gcall_start",
        "eyJ0Ijoic3RhcnQiLCJ2IjoxLCJnaWQiOiJncnAtZ29sZGVuIiwiY2FsbF9pZCI6IkFRSURCQVVHQndnSkNnc01EUTRQRUEiLCJob3N0X2dlbiI6MSwiaG9zdF9yZWYiOnsiaG9sZGVyX2RpZCI6ImRpZDprZXk6ejZNa25uY1ZtZWRwaGdEU2JRb1lMeU5hRlJGZEIyV3R5anhjRmRLVjZ6VGZjWXZzIiwiZGV2aWNlX3Nsb3QiOjMsInBlcm1pdF9kaWdlc3QiOiJkaWdlc3QtZ29sZGVuIn0sInN0YXJ0ZXJfZGlkIjoiZGlkOmtleTp6Nk1rbm5jVm1lZHBoZ0RTYlFvWUx5TmFGUkZkQjJXdHlqeGNGZEtWNnpUZmNZdnMiLCJzdGFydGVkX21zIjoxODAwMDAwMDAwMDAwLCJrIjozLCJtYXhfcGFydGljaXBhbnRzIjo4LCJiYWNrdXBzIjpbeyJob2xkZXJfZGlkIjoiZGlkOmtleTp6Nk1rdm1BNHFINzJGTjd2OXpjOXkzZTl3clJqdVRvZVo2ZVZVcGMxY2Y3OHlrYmMiLCJkZXZpY2Vfc2xvdCI6MywicGVybWl0X2RpZ2VzdCI6ImRpZ2VzdC1nb2xkZW4ifV0sIm1ldGFfZnJvbnRpZXIiOiJmcm9udGllci1nb2xkZW4ifQ",
        "WCu71G0ozh4bpPHC3i-D4jymwynOo4BAMsZ1XLt7ZPIpAG99vlyZt9S8kdrcNjG0-p-X5aYjmIxRJcVgWa01BQ",
    ),
    (
        "gcall_roster",
        "eyJ0Ijoicm9zdGVyIiwidiI6MSwiZ2lkIjoiZ3JwLWdvbGRlbiIsImNhbGxfaWQiOiJBUUlEQkFVR0J3Z0pDZ3NNRFE0UEVBIiwiaG9zdF9nZW4iOjEsImhvc3RfZGlkIjoiZGlkOmtleTp6Nk1rbm5jVm1lZHBoZ0RTYlFvWUx5TmFGUkZkQjJXdHlqeGNGZEtWNnpUZmNZdnMiLCJlcG9jaCI6MiwiZW50cmllcyI6W3sicyI6MSwiZCI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiaSI6MTgwMDAwMDAwMDAwNSwibiI6NDIsImciOiJjMmxuIn1dfQ",
        "XogXxBw9yix_HdH4nxTrg1QIfiBrpTgExkrSRsNw8pDde7CvLz0Z7WYg9yzlxs9doajgnCFL7NZCE2-3CJ_DAw",
    ),
    (
        "gcall_leave",
        "eyJ0IjoibGVhdmUiLCJ2IjoxLCJnaWQiOiJncnAtZ29sZGVuIiwiY2FsbF9pZCI6IkFRSURCQVVHQndnSkNnc01EUTRQRUEiLCJob3N0X2dlbiI6MSwiZGlkIjoiZGlkOmtleTp6Nk1rdm1BNHFINzJGTjd2OXpjOXkzZTl3clJqdVRvZVo2ZVZVcGMxY2Y3OHlrYmMiLCJpc3N1ZWRfbXMiOjE4MDAwMDAwMDAwMDl9",
        "k3iS0z67bjr65Wt0x0fu-aELCDPdH3KkF2srXGVKiPwD9WBwo_WAnEaoMAtu4d2Pj9Snl0T-bZImJznJLXeaDQ",
    ),
    (
        "gcall_end",
        "eyJ0IjoiZW5kIiwidiI6MSwiZ2lkIjoiZ3JwLWdvbGRlbiIsImNhbGxfaWQiOiJBUUlEQkFVR0J3Z0pDZ3NNRFE0UEVBIiwiaG9zdF9nZW4iOjEsImhvc3RfZGlkIjoiZGlkOmtleTp6Nk1rbm5jVm1lZHBoZ0RTYlFvWUx5TmFGUkZkQjJXdHlqeGNGZEtWNnpUZmNZdnMiLCJyZWFzb24iOiJob3N0X3JvdGF0aW5nIiwibmV4dF9ob3N0Ijp7ImhvbGRlcl9kaWQiOiJkaWQ6a2V5Ono2TWt2bUE0cUg3MkZON3Y5emM5eTNlOXdyUmp1VG9lWjZlVlVwYzFjZjc4eWtiYyIsImRldmljZV9zbG90IjozLCJwZXJtaXRfZGlnZXN0IjoiZGlnZXN0LWdvbGRlbiJ9fQ",
        "rT9maK8UEVd6iA3PAYM2qkUtdZeqTh5pEy9bdNn9MmexZ1NKJdtMXmz9bIutaA28M7dX3gWecOj6lefiaf07CQ",
    ),
    (
        "gcall_host_request",
        "eyJ0IjoiaG9zdF9yZXF1ZXN0IiwidiI6MSwiZ2lkIjoiZ3JwLWdvbGRlbiIsImNhbGxfaWQiOiJBUUlEQkFVR0J3Z0pDZ3NNRFE0UEVBIiwic3RhcnRlcl9kaWQiOiJkaWQ6a2V5Ono2TWtubmNWbWVkcGhnRFNiUW9ZTHlOYUZSRmRCMld0eWp4Y0ZkS1Y2elRmY1l2cyIsImhvc3QiOnsiaG9sZGVyX2RpZCI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiZGV2aWNlX3Nsb3QiOjMsInBlcm1pdF9kaWdlc3QiOiJkaWdlc3QtZ29sZGVuIn0sImJhY2t1cHMiOltdLCJleHBlY3RlZCI6NCwiaXNzdWVkX21zIjoxODAwMDAwMDAwMDAwfQ",
        "eVNViaihpJngjuSAx725X13IdPYooOs9EaTbCDTsdRED6FVS15igEDNNBMnWDJcXyTMXy1PXl3SdDYn-L3gECQ",
    ),
    (
        "gcall_host_refusal",
        "eyJ0IjoiaG9zdF9yZWZ1c2FsIiwidiI6MSwiZ2lkIjoiZ3JwLWdvbGRlbiIsImNhbGxfaWQiOiJBUUlEQkFVR0J3Z0pDZ3NNRFE0UEVBIiwic3RhcnRlcl9kaWQiOiJkaWQ6a2V5Ono2TWtubmNWbWVkcGhnRFNiUW9ZTHlOYUZSRmRCMld0eWp4Y0ZkS1Y2elRmY1l2cyIsImhvc3QiOnsiaG9sZGVyX2RpZCI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiZGV2aWNlX3Nsb3QiOjMsInBlcm1pdF9kaWdlc3QiOiJkaWdlc3QtZ29sZGVuIn0sInJlYXNvbiI6ImJ1c3kifQ",
        "0-Ut74VbGf5ChZfKONalPTUApJvB87l1LTUTqbZYPf-1IYJr1i3dQGb5y4yNCrK0DGaS7qQZjRlVA1psbHtqAg",
    ),
    (
        "gcall_note",
        "eyJ0Ijoibm90ZSIsInYiOjEsImRpZCI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiZGV2IjoiQndnSkNnc01EUTRQRUJFU0V4UVZGaGNZR1JvYkhCMGVIeUFoSWlNa0pTWSIsImVuZHBvaW50IjoiQ1FvTERBME9EeEFSRWhNVUZSWVhHQmthR3h3ZEhoOGdJU0lqSkNVbUp5ZyIsImlzc3VlZF9tcyI6MTgwMDAwMDAwMDAwMH0",
        "TsyR_uAWYrT7bCJYaRqWGXRtyRDo2a4xjfxCKmOgq-07xjN5BwmQgDxT3Xcoin50G0p190xTJHI4wiUTO_JsBg",
    ),
    (
        "gcall_status",
        "eyJ0IjoiaG9zdF9zdGF0dXMiLCJ2IjoxLCJnaWQiOiJncnAtZ29sZGVuIiwiaG9zdF9yZWYiOnsiaG9sZGVyX2RpZCI6ImRpZDprZXk6ejZNa25uY1ZtZWRwaGdEU2JRb1lMeU5hRlJGZEIyV3R5anhjRmRLVjZ6VGZjWXZzIiwiZGV2aWNlX3Nsb3QiOjMsInBlcm1pdF9kaWdlc3QiOiJkaWdlc3QtZ29sZGVuIn0sImNsYXNzIjoicGhvbmUiLCJwb3dlciI6eyJiYXR0ZXJ5Ijo4MH0sIm5ldHdvcmsiOiJtZXRlcmVkIiwidGhlcm1hbCI6ImxpZ2h0IiwiYnVzeSI6ZmFsc2UsImxvYWQiOjEwLCJtYXhfcGFydGljaXBhbnRzIjo2LCJpc3N1ZWRfbXMiOjE4MDAwMDAwMDAwMDAsInR0bF9tcyI6MzAwMDB9",
        "Zx2E6HkCFiIw9_rtGFNXXHYYLoaBW7vH9DJqj0DE5HAzQ8aKFwq5wUx26AS8fnhuql9BIJE0uPBFb68moYmCDQ",
    ),
    (
        "picture_deletion",
        "eyJ2IjoxLCJvd25lciI6ImRpZDprZXk6ejZNa25uY1ZtZWRwaGdEU2JRb1lMeU5hRlJGZEIyV3R5anhjRmRLVjZ6VGZjWXZzIiwicGljdHVyZV9oYXNoIjoiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYiIsInNjb3BlIjoiY2FjaGVkLXByb2ZpbGUtcGljdHVyZSIsImNvbnNlbnQiOnRydWV9",
        "XHtxmn9rV6jJ2jTnvT5MmN_hR537KbztCFL3C1MdvK77zObsBN8WNOGtO2MbO9W1kc0wWFIB-ZRY7kMjdwahAQ",
    ),
];
const FRAMES: &[(&str, &str)] = &[
    ("ringing", "0000000f7b2274223a2272696e67696e67227d"),
    ("busy", "0000000c7b2274223a2262757379227d"),
    ("hangup", "0000000e7b2274223a2268616e677570227d"),
    (
        "reject",
        "0000002b7b2274223a2272656a656374222c22726561736f6e223a2263616c6c73206e6f7420616c6c6f776564227d",
    ),
    (
        "video",
        "0000001b7b2274223a22766964656f222c2263616d657261223a747275657d",
    ),
    (
        "notice",
        "0000016f7b2274223a226e6f74696365222c226e6f74696365223a7b227061796c6f6164223a2265794a32496a6f784c434a6d636d3974496a6f695a476c6b4f6d746c655470364e6b3172626d356a566d316c5a48426f5a305254596c467657557835546d4647556b5a6b516a4a5864486c7165474e475a4574574e6e70555a6d4e5a646e4d694c434a3062794936496d52705a4470725a586b36656a5a4e61335a745154527853446379526b3433646a6c36597a6c354d32553564334a53616e5655623256614e6d56575658426a4d574e6d4e7a683561324a6a49697769596d78765932746c5a43493664484a315a537769595851694f6a45344d4441774d4441774d4441774d444239222c227369676e6174757265223a227a62545272734868465454626d42594453487039524946485f6d73464b72623350437830546a785138585650474f2d5775693667346a445a4a41524b4943426564364c50566166683439792d7730555a425a51624377227d7d",
    ),
    (
        "devices",
        "000001727b2274223a2264657669636573222c226c697374223a7b227061796c6f6164223a2265794a32496a6f784c434a6b615751694f694a6b61575136613256354f6e6f3254577475626d4e576257566b6347686e52464e695557395a54486c4f59555a53526d52434d6c64306557703459305a6b53315932656c526d59316c3263794973496e4e6c635349364d5467774d4441774d4441774d4441774d4377695a47563261574e6c63794936573373695a47563261574e6c496a6f695156464a52454a42565564436432644b5132647a545552524e464246516b565452586852566b5a6f59316c48556d3969534549775a55683551534973496e4a6c62474635496a70756457787366563139222c227369676e6174757265223a224d5550364f7666376e4b716759546871337546724655673567365363724b4e584b6579566d67334e7643445834694b774b77627a6932515743317756355361536841785a59593534326557464864436a652d65794441227d7d",
    ),
    (
        "accept",
        "000000417b2274223a22616363657074222c2272656e657765645f6772616e74223a6e756c6c2c22766964656f223a7b226d61785f706978656c73223a3932313630307d7d",
    ),
    (
        "decline",
        "000000207b2274223a226465636c696e65222c22726561736f6e223a226c61746572227d",
    ),
    (
        "cancel",
        "0000002c7b2274223a2263616e63656c222c22726561736f6e223a22616e7377657265645f656c73657768657265227d",
    ),
    (
        "tokens",
        "0000014d7b2274223a22746f6b656e73222c226772616e7473223a5b7b227061796c6f6164223a2265794a32496a6f784c434a6b615751694f694a6b61575136613256354f6e6f3254577475626d4e576257566b6347686e52464e695557395a54486c4f59555a53526d52434d6c64306557703459305a6b53315932656c526d59316c3263794973496d526c646d6c6a5a534936496b46525355524351565648516e646e536b4e6e633031455554525152554a465530563455565a4761474e5a52314a76596b68434d475649655545694c434a70595851694f6a45344d4441774d4441774d444239222c227369676e6174757265223a227376475a584155695969623147506f6e4463674e614b56442d6f564957682d784e43565632657a3730636774363065735051555048354d4d48375241453277354b50597258613463453543466f557437755766654377227d5d7d",
    ),
    (
        "group_letter",
        "000002a67b2274223a2267726f75705f6c6574746572222c226174746573746174696f6e223a7b227061796c6f6164223a2265794a32496a6f784c434a6b615751694f694a6b61575136613256354f6e6f3254577475626d4e576257566b6347686e52464e695557395a54486c4f59555a53526d52434d6c64306557703459305a6b53315932656c526d59316c3263794973496d526c646d6c6a5a534936496b46525355524351565648516e646e536b4e6e633031455554525152554a465530563455565a4761474e5a52314a76596b68434d475649655545694c434a70595851694f6a45344d4441774d4441774d444239222c227369676e6174757265223a227376475a584155695969623147506f6e4463674e614b56442d6f564957682d784e43565632657a3730636774363065735051555048354d4d48375241453277354b50597258613463453543466f557437755766654377227d2c226772616e74223a7b227061796c6f6164223a2265794a32496a6f784c434a6b615751694f694a6b61575136613256354f6e6f3254577475626d4e576257566b6347686e52464e695557395a54486c4f59555a53526d52434d6c64306557703459305a6b53315932656c526d59316c3263794973496d526c646d6c6a5a534936496b466e5455564355566c495130467253304e33643035455a7a685252564a4a56455a4356566447654764615232687a593068534e475a4a513055694c434a70595851694f6a45344d4441774d4441774d444239222c227369676e6174757265223a22324d47334b50374848534153636272674b6e4361597042694c6f706156657436747057586e534d436e73517859656a37637a314e5739677874504276637a6967725836304f3435425a4f476c77634f5a485776574341227d2c226c6574746572223a226247563064475679227d",
    ),
    (
        "gcall_heartbeat",
        "0000001b7b2274223a22686561727462656174222c2265706f6368223a377d",
    ),
    (
        "gcall_active",
        "000000267b2274223a22616374697665222c2267656e223a322c22736c6f7473223a5b312c342c355d7d",
    ),
];
const LINKS: &[(&str, &str)] = &[
    (
        "qr_none",
        "OSVL1:AEBQIBIGA4EASCQLBQGQ4DYQCEJBGFAVCYLRQGI2DMOB2HQ7EAQSECIJBEEQSCIJBEEQSCIJBEEQSCIAAPUAA",
    ),
    (
        "qr_custom",
        "OSVL1:AEBQIBIGA4EASCQLBQGQ4DYQCEJBGFAVCYLRQGI2DMOB2HQ7EAQSECIJBEEQSCIJBEEQSCIJBEEQSCIAAPUOAGTIOR2HA4Z2F4XXEZLMMF4S4ZLYMFWXA3DFFZ2GK43U",
    ),
    (
        "proof_scanner",
        "fd9d02737d49fab747db52cc4c809d1410e6dde03c2728b6a364895136f70c25",
    ),
    (
        "proof_displayer",
        "7a784ed7fa36c899cf8224beca247dc2a4cf76a8545025a17380f954aec01cdc",
    ),
    ("confirm_code", "957 831"),
];
const RANDOM_TOKEN: (&str, &str) = (
    "eyJ2IjoyLCJpZCI6IlJ3VEZJeU9TTElHVkMzdUQ5XzJTU2ciLCJpc3MiOiJkaWQ6a2V5Ono2TWtubmNWbWVkcGhnRFNiUW9ZTHlOYUZSRmRCMld0eWp4Y0ZkS1Y2elRmY1l2cyIsImhvbGRlciI6ImRpZDprZXk6ejZNa3ZtQTRxSDcyRk43djl6Yzl5M2U5d3JSanVUb2VaNmVWVXBjMWNmNzh5a2JjIiwiY2FwIjoibWVzc2FnZSIsImlhdCI6MTgwMDAwMDAwMCwiZXhwIjoxODAwMDAzNjAwfQ",
    "phVEhKU7DsCJ_2Oipfz8woH1W4PCA033HPhuYKFwuRH_oN-QksNuXlkRwZK3FRML9_etcnYO2TSAotFEF19FCA",
);
const RANDOM_TICKET: &str = "OSVC2:PPJKPPDRZ3WCT3WAROFVBZTIR2T7OCJJECUQCMQ3TTSVWNVGKJUACAQDAQCQMBYIBEFAWDANBYHRAEISCMKBKFQXDAMRUGY4DUPB6IHFV2WIGK6JBBK4VZSHN5WGIZLODJUHI5DQOM5C6L3SMVWGC6JOMV4GC3LQNRSS45DFON2JNV6W6JPZ2AMFJOBLYIBW77QP3G647VAFF4BU5AXG7U6G3O5434IIPLYB2U4P7VOWJYMUGLUE6PBGTUDWRMM7RCWSMRYCVZO7E27TBI";
const RANDOM_LINK_GRANT: &str = "KsKxyoU0_1DkZMw4v7kPbFjeUXvctrPXOkhkFgYSMu-3tjlYJ7A4T00xPOBy";
const RANDOM_GCALL_JOIN: (&str, &str) = (
    "eyJ0Ijoiam9pbiIsInYiOjEsImdpZCI6ImdycC1nb2xkZW4iLCJjYWxsX2lkIjoiQVFJREJBVUdCd2dKQ2dzTURRNFBFQSIsImhvc3RfZ2VuIjoxLCJkaWQiOiJkaWQ6a2V5Ono2TWt2bUE0cUg3MkZON3Y5emM5eTNlOXdyUmp1VG9lWjZlVlVwYzFjZjc4eWtiYyIsImlzc3VlZF9tcyI6MTgwMDAwMDAwMDAwMCwibm9uY2UiOjE2MjY1MjQ3MDQ1MDUyNTYzMTc2fQ",
    "OrbCVZzdpwGslZEe-__Hj_6vcub3Vivrtm59dhCY7FlUGI1J0Gyq0nl4qoxKKNPJR04YEDZUg5_bg0Wj8hrSCA",
);

#[test]
#[ignore = "generator"]
fn dump() {
    println!("const BLOBS: &[(&str, &str, &str)] = &[");
    for (n, b) in blobs() {
        println!("    (\"{n}\", \"{}\", \"{}\"),", b.payload, b.signature);
    }
    println!("];\nconst FRAMES: &[(&str, &str)] = &[");
    for (n, f) in frames() {
        println!("    (\"{n}\", \"{}\"),", hex(&f));
    }
    println!("];\nconst LINKS: &[(&str, &str)] = &[");
    for (n, t) in link_texts().into_iter().chain(link_proofs()) {
        println!("    (\"{n}\", \"{t}\"),");
    }
    println!("];");
    let g = issue_token(&a(), b().did(), Cap::Message, T0, 3600);
    println!(
        "const RANDOM_TOKEN: (&str, &str) = (\"{}\", \"{}\");",
        g.payload, g.signature
    );
    println!(
        "const RANDOM_TICKET: &str = \"{}\";",
        issue_contact_ticket(
            &a(),
            dev(1),
            "Golden",
            Some("https://relay.example.test".into()),
            T0,
            600
        )
        .to_text()
    );
    println!(
        "const RANDOM_LINK_GRANT: &str = \"{}\";",
        proto::link::seal_link_grant(&[9u8; 16], &dev(10), &dev(20), b"golden grant body")
    );
    let (_, j) = gcall::sign_join(&b(), GID, CALL_ID, 1, T0 * 1000);
    println!(
        "const RANDOM_GCALL_JOIN: (&str, &str) = (\"{}\", \"{}\");",
        j.payload, j.signature
    );
}

#[test]
fn deterministic_blobs_are_byte_identical() {
    let built = blobs();
    assert_eq!(built.len(), BLOBS.len());
    for ((name, blob), (gname, payload, sig)) in built.iter().zip(BLOBS) {
        assert_eq!(name, gname);
        assert_eq!(&blob.payload, payload, "{name} payload");
        assert_eq!(&blob.signature, sig, "{name} signature");
    }
}

fn captured(name: &str) -> SignedBlob {
    let (_, p, s) = BLOBS.iter().find(|(n, _, _)| *n == name).unwrap();
    SignedBlob {
        payload: p.to_string(),
        signature: s.to_string(),
    }
}

#[test]
fn captured_blobs_open() {
    let (a, b) = (a(), b());
    assert_eq!(
        verify_attestation(&captured("attestation"))
            .unwrap()
            .device_key()
            .unwrap(),
        dev(1)
    );
    let list = verify_device_list(&captured("device_list"), a.did()).unwrap();
    assert_eq!(list.devices.len(), 2);
    assert_eq!(list.seq, T0 * 1000);
    let n = verify_block_notice(&captured("block_notice"), b.did()).unwrap();
    assert!(n.blocked && n.from == a.did());
    assert!(
        !verify_block_notice(&captured("unblock_notice"), b.did())
            .unwrap()
            .blocked
    );
    assert_eq!(gcall::open_start(&captured("gcall_start")).unwrap().k, 3);
    // The entry carries a made-up Join signature, so opening refuses it today; the host signature and
    // payload bytes of the roster are still frozen by `deterministic_blobs_are_byte_identical`.
    assert_eq!(
        gcall::open_roster(&captured("gcall_roster")),
        Err(Error::Decode)
    );
    gcall::open_leave(&captured("gcall_leave")).unwrap();
    assert_eq!(
        gcall::open_end(&captured("gcall_end")).unwrap().reason,
        gcall::EndReason::HostRotating
    );
    gcall::open_host_request(&captured("gcall_host_request")).unwrap();
    gcall::open_host_refusal(&captured("gcall_host_refusal")).unwrap();
    assert_eq!(
        gcall::open_note(&captured("gcall_note"))
            .unwrap()
            .dev_bytes(),
        Some(dev(7))
    );
    let st = gcall_status::open_status_for(&captured("gcall_status"), GID, T0 * 1000 + 1).unwrap();
    assert_eq!(st.load, 10);
    profile::verify_picture_deletion(&captured("picture_deletion"), a.did()).unwrap();
    // A flipped signature byte is refused.
    let mut bad = captured("attestation");
    bad.signature = captured("device_list").signature;
    assert_eq!(verify_attestation(&bad), Err(Error::BadSignature));
}

#[test]
fn captured_frames_encode_and_decode_both_ways() {
    let built = frames();
    assert_eq!(built.len(), FRAMES.len());
    for ((name, bytes), (gname, h)) in built.iter().zip(FRAMES) {
        assert_eq!(name, gname);
        assert_eq!(&hex(bytes), h, "{name} frame bytes");
        let raw = unhex(h);
        if name.starts_with("gcall") {
            let (m, used) = gcall::decode_control(&raw).unwrap().unwrap();
            assert_eq!(used, raw.len());
            assert_eq!(
                &gcall::encode_control(&m).unwrap(),
                bytes,
                "{name} re-encode"
            );
        } else {
            let (m, used) = decode_frame(&raw).unwrap().unwrap();
            assert_eq!(used, raw.len());
            assert_eq!(&encode_frame(&m), bytes, "{name} re-encode");
        }
    }
    // A short buffer asks for more.
    assert_eq!(decode_frame(&unhex(FRAMES[0].1)[..3]).unwrap(), None);
}

#[test]
fn link_text_proofs_and_code() {
    let mut built = link_texts();
    built.extend(link_proofs());
    assert_eq!(built.len(), LINKS.len());
    for ((name, v), (gname, g)) in built.iter().zip(LINKS) {
        assert_eq!(name, gname);
        assert_eq!(v, g, "{name}");
    }
    for (n, t) in LINKS.iter().take(2) {
        let qr = proto::link::LinkQr::from_text(t).unwrap();
        assert_eq!(&qr.to_text(), t, "{n}");
        assert_eq!(qr.device, dev(3));
    }
    use proto::link::{LinkRole, verify_link_proof};
    let (s, e, n) = ([9u8; 16], dev(10), dev(20));
    verify_link_proof(&s, &e, &n, LinkRole::Scanner, &e, &unhex(LINKS[2].1)).unwrap();
}

#[test]
fn captured_random_items_verify() {
    let (a, b) = (a(), b());
    let tok = SignedBlob {
        payload: RANDOM_TOKEN.0.into(),
        signature: RANDOM_TOKEN.1.into(),
    };
    let g = verify_token(
        &tok,
        a.did(),
        b.did(),
        Cap::Message,
        0,
        T0 + 10,
        &HashSet::new(),
    )
    .unwrap();
    assert_eq!((g.iat, g.exp), (T0, T0 + 3600));
    assert_eq!(
        verify_token(
            &tok,
            a.did(),
            b.did(),
            Cap::Call,
            0,
            T0 + 10,
            &HashSet::new()
        ),
        Err(Error::BadCapability)
    );
    assert_eq!(
        verify_token(
            &tok,
            a.did(),
            b.did(),
            Cap::Message,
            0,
            T0 + 3600,
            &HashSet::new()
        ),
        Err(Error::Expired)
    );

    let ticket = ContactTicket::from_text(RANDOM_TICKET).unwrap();
    let claim = ticket.verify(T0 + 1).unwrap();
    assert_eq!(claim.iss, a.did());
    assert_eq!(ticket.name, "Golden");
    assert_eq!(ticket.to_text(), RANDOM_TICKET);

    let body =
        proto::link::open_link_grant(&[9u8; 16], &dev(10), &dev(20), RANDOM_LINK_GRANT).unwrap();
    assert_eq!(body, b"golden grant body");
    assert!(
        proto::link::open_link_grant(&[9u8; 16], &dev(10), &dev(21), RANDOM_LINK_GRANT).is_err()
    );

    let join = SignedBlob {
        payload: RANDOM_GCALL_JOIN.0.into(),
        signature: RANDOM_GCALL_JOIN.1.into(),
    };
    assert_eq!(gcall::open_join(&join).unwrap().did, b.did());
}
