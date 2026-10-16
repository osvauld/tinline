//! Golden wire vectors for `proto`, frozen BEFORE the dependency upgrades (ed25519-dalek 3, sha2 0.11,
//! hkdf/hmac 0.13, aes-gcm 0.11, rand 0.10). Must pass UNCHANGED afterwards.
//! Regenerated 2026-10-15 for the one signing-input rule (`cryptography::sign::input`) and BLAKE3 link
//! keys/proofs, and again for the `Signed<T>` envelope (postcard body || signature, one domain per type,
//! no `v` fields; pre-release, no back-compat), and for `proto::issued` (tokens and picture
//! permits as `Issued<C>`, unix ms). 2026-10-16: `gcall_end` regenerated after `EndReason::HostRotating`
//! (never sent) was removed; the vector now carries `HostLeaving`; `accept` regenerated for the codec lists
//! in `VideoOffer` (`docs/design/video-hw-vp8.md`) and again for `VideoOffer.max_fps`
//! (`docs/design/video-adaptation.md`) and for `VideoOffer.hw_decode`/`hw_encode`. DIDs and the QR text layouts did not change; the
//! `OSVC2:` ticket is now the envelope's bytes. Signed items are stored as hex of `Signed::to_bytes()`.
//!
//! Produced by `cargo test -p proto --test golden_wire -- --ignored --nocapture dump` on base 0e1eecb,
//! pasted into the tables below. Identities come from the public BIP-39 test phrases (synthetic).
//! Deterministic items (Ed25519 is deterministic, JSON field order is the struct order) are rebuilt and
//! compared byte for byte, then opened again. Items with random ids/nonces (tokens, tickets, joins, sealed
//! link grants) are checked on the decode/verify side from captured bytes.

use identity::{Identity, recover};
use proto::*;

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

/// An issued statement with a fixed nonce (the real `issue` draws a random one).
fn issued<C: IssuedClaim>(by: &Identity, sub: Subject, claim: C) -> Issued<C> {
    Issued { iss: by.did().into(), sub, iat_ms: T0 * 1000, exp_ms: T0 * 1000 + 3_600_000, nonce: [5; 16], claim }
}

fn host(i: &Identity) -> gcall::HostRef {
    gcall::HostRef {
        holder_did: i.did().into(),
        device_slot: 3,
        permit_digest: "digest-golden".into(),
    }
}

/// Every deterministic signed envelope (`to_bytes()`), by name.
fn blobs() -> Vec<(&'static str, Vec<u8>)> {
    let (a, b) = (a(), b());
    let start = gcall::CallStart {
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
    // The entry carries a real Join signature (Ed25519 is deterministic), so the roster opens.
    let join = gcall::Join {
        gid: GID.into(),
        call_id: CALL_ID.into(),
        host_gen: 1,
        did: b.did().into(),
        issued_ms: T0 * 1000 + 5,
        nonce: 42,
    };
    let join_env = Signed::sign(&join, &b).unwrap();
    let roster = gcall::Roster {
        gid: GID.into(),
        call_id: CALL_ID.into(),
        host_gen: 1,
        host_did: a.did().into(),
        epoch: 2,
        entries: vec![gcall::RosterEntry::new(1, &join, &join_env)],
    };
    let end = gcall::End {
        gid: GID.into(),
        call_id: CALL_ID.into(),
        host_gen: 1,
        host_did: a.did().into(),
        reason: gcall::EndReason::HostLeaving,
        next_host: Some(host(&b)),
    };
    let req = gcall::HostRequest {
        gid: GID.into(),
        call_id: CALL_ID.into(),
        starter_did: a.did().into(),
        host: host(&b),
        backups: vec![],
        expected: 4,
        issued_ms: T0 * 1000,
    };
    let refusal = gcall::HostRefusal {
        gid: GID.into(),
        call_id: CALL_ID.into(),
        starter_did: a.did().into(),
        host: host(&b),
        reason: "busy".into(),
    };
    let status = gcall_status::CallHostStatus {
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
        ("attestation", attest(&a, dev(1), T0).to_bytes()),
        (
            "device_list",
            sign_device_list(
                &a,
                &[
                    (dev(1), None),
                    (dev(50), Some("https://relay.example.test".into())),
                ],
                T0 * 1000,
            )
            .to_bytes(),
        ),
        (
            "block_notice",
            sign_block_notice(&a, b.did(), true, T0 * 1000).to_bytes(),
        ),
        (
            "unblock_notice",
            sign_block_notice(&a, b.did(), false, T0 * 1000 + 1).to_bytes(),
        ),
        ("gcall_start", Signed::sign(&start, &a).unwrap().to_bytes()),
        (
            "gcall_roster",
            Signed::sign(&roster, &a).unwrap().to_bytes(),
        ),
        (
            "gcall_leave",
            gcall::sign_leave(&b, GID, CALL_ID, 1, T0 * 1000 + 9)
                .unwrap()
                .to_bytes(),
        ),
        ("gcall_end", Signed::sign(&end, &a).unwrap().to_bytes()),
        (
            "gcall_host_request",
            Signed::sign(&req, &a).unwrap().to_bytes(),
        ),
        (
            "gcall_host_refusal",
            Signed::sign(&refusal, &b).unwrap().to_bytes(),
        ),
        (
            "gcall_note",
            gcall::sign_note(&b, &dev(7), &dev(9), T0 * 1000).to_bytes(),
        ),
        (
            "gcall_status",
            Signed::sign(&status, &a).unwrap().to_bytes(),
        ),
        (
            "picture_deletion",
            Signed::sign(&issued(&a, Subject::Anyone, profile::PictureDeletion {
                picture_hash: "ab".repeat(32),
                scope: profile::PICTURE_DELETE_SCOPE.into(),
            }), &a)
            .unwrap()
            .to_bytes(),
        ),
        (
            "grant",
            Signed::sign(&issued(&a, Subject::Did(b.did().into()), Cap::Call), &a)
                .unwrap()
                .to_bytes(),
        ),
    ]
}

/// Encoded frames, by name.
fn frames() -> Vec<(&'static str, Vec<u8>)> {
    let (a, _) = (a(), b());
    let att = attest(&a, dev(1), T0);
    // Any envelope serialises the same way; an attestation stands in for the grant slots.
    let as_grant = |s: &SignedAttestation| Signed::<Grant>::from_bytes(&s.to_bytes()).unwrap();
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
                    max_fps: 30,
                    decode: vec![VideoCodec::Av1, VideoCodec::Vp8],
                    encode: vec![VideoCodec::Vp8, VideoCodec::Av1],
                    hw_decode: vec![VideoCodec::Vp8],
                    hw_encode: vec![VideoCodec::Vp8],
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
                grants: vec![as_grant(&att)],
            }),
        ),
        (
            "group_letter",
            encode_frame(&Msg::GroupLetter {
                attestation: att,
                grant: as_grant(&attest(&a, dev(2), T0)),
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
const BLOBS: &[(&str, &str)] = &[
    ("attestation", "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a5466635976730102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2080a4a7da061acb5d5cdf9553e912806b2790a0b86452677ac12a2ebd3a8f797d23a13612c144c559d26d7de427d1fa5b6a2a050ddf6bf27a8fd9e6de634700a1a40ab4c00b"),
    ("device_list", "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a54666359767380a0f1c2b134020102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f200032333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f5051011a68747470733a2f2f72656c61792e6578616d706c652e7465737400b153712230397123a97636e6b3db45736f7a66323fc5faae6913ee9740465b887558ea4c544247490238d3a9a4b0fb144961299ca30b06c2ce078c477c9f7f07"),
    ("block_notice", "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b62630180a0f1c2b1342cd9180926c8d36e7e0268c12206eebf8bead05c7b8da862aa074aac28200bd1f95cddacd1126edb0b10dec0fc52a1f670eac3d3acb8c9203b0663ef02cdaf0b"),
    ("unblock_notice", "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b62630081a0f1c2b13448ced4abf6888b0a97c877cba7b832cffd79f6704de08ab961133d93c9821d8fc679e4a59db0bbfa4e70cdf92830b7cd3733cb3d6039634e518dacd8ea70b40b"),
    ("gcall_start", "0a6772702d676f6c64656e1641514944424155474277674a4367734d44513450454101386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673030d6469676573742d676f6c64656e386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a54666359767380a0f1c2b134030801386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b6263030d6469676573742d676f6c64656e0f66726f6e746965722d676f6c64656ed5daba016afdc1c7305b8f07f7eae4f46378e45c7c49b48da018b1e09b71f1a705420129859e965b2834d1e72b4ef90e2aca237da0b6affdbb0fd531c5d23f02"),
    ("gcall_roster", "0a6772702d676f6c64656e1641514944424155474277674a4367734d44513450454101386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673020101386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b626385a0f1c2b1342a40eefe73f47defa03630c5c62346f6bac02eaabe2255fc06962293f18c7c240c4543c063a804d1b8797cc3645a454183bdc5313c9a568f555b2df0b6d632a0250bec0ba7a551a24689a7826ed470a0b068b7c0d2b73641f3a3b0d57508d9c01775ea7b090c5a8d27b18f300b4933d388cc751026dec91b4d26fe7e9441b706d60e"),
    ("gcall_leave", "0a6772702d676f6c64656e1641514944424155474277674a4367734d44513450454101386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b626389a0f1c2b1347221598af39a505448c53505053595836beede86576e20e2937621930435887054146fdb3e11e44a60c4d99ca596561d39d3a37d2cdbabe588e517aa5652510c"),
    ("gcall_end", "0a6772702d676f6c64656e1641514944424155474277674a4367734d44513450454101386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a5466635976730101386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b6263030d6469676573742d676f6c64656ef0185c509896db558166e73067b244122a06d0caae36ed1de454dc7943a03d29e8b7da28dc59636acd4531e2427375d425c41b152c373b0def467f1ec37cb20c"),
    ("gcall_host_request", "0a6772702d676f6c64656e1641514944424155474277674a4367734d445134504541386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b6263030d6469676573742d676f6c64656e000480a0f1c2b13440b1d9bf93e5fca63db91884775397b7bd8d798335bb9d13a8a04f66ed0613a2a8b8288e23ad3f6c65ef79f708e1bb38a1f5bfe59ae63ad5ce651f1bdb46e80d"),
    ("gcall_host_refusal", "0a6772702d676f6c64656e1641514944424155474277674a4367734d445134504541386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b6263030d6469676573742d676f6c64656e0462757379cfec59974c18731a02169ce612f3fb9731e6747a9c14b90bf79ecab2f65be1dc93951c5b90ff545f6b08773e773798d437f6968c958db4ab7729f9184eda6d06"),
    ("gcall_note", "386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b62630708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20212223242526090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20212223242526272880a0f1c2b1349e694d67dd5217f691d1b98f51080ef84efb387272cddb0e21dc27ddac4b2b21cae2055af6f84d11b246b0cf481e5429d6b043460004cc29ea5f90f2aacd7f0b"),
    ("gcall_status", "0a6772702d676f6c64656e386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a546663597673030d6469676573742d676f6c64656e0202500101000a0680a0f1c2b134b0ea01d9853878859c8db48d72b658c27498a164fde2b5375559eb02765a45dfb7fc9daf2a098ae6d5ea4415fbff36e10fd6671d182ff997f67503379914fec52d0806"),
    ("picture_deletion", "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a5466635976730180a0f1c2b13480fdccc4b134050505050505050505050505050505054061626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162166361636865642d70726f66696c652d70696374757265e3b092739a261335b4ab79ba46d93b6bd8ca00840adc3edadce11a0993673805f1413c0bd928e875aa547fc7d1196ece173b77246edb7fc117499b311e7dfb0d"),
    ("grant", "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a54666359767300386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b626380a0f1c2b13480fdccc4b134050505050505050505050505050505050198328899159acf9653bf4bf959265ef90d42199c5f5b01a6028bb762613c47d46c8f3fedbe7818a50ff2e16e016e961c2b15ef2a96bf4cbf1bc21fb6c19e3401"),
];
const FRAMES: &[(&str, &str)] = &[
    ("ringing", "0000000f7b2274223a2272696e67696e67227d"),
    ("busy", "0000000c7b2274223a2262757379227d"),
    ("hangup", "0000000e7b2274223a2268616e677570227d"),
    ("reject", "0000002b7b2274223a2272656a656374222c22726561736f6e223a2263616c6c73206e6f7420616c6c6f776564227d"),
    ("video", "0000001b7b2274223a22766964656f222c2263616d657261223a747275657d"),
    ("notice", "000001117b2274223a226e6f74696365222c226e6f74696365223a224f4752705a4470725a586b36656a5a4e6132357559315a745a5752776147644555324a5262316c4d65553568526c4a475a45497956335235616e686a526d524c566a5a3656475a6a57585a7a4f4752705a4470725a586b36656a5a4e61335a745154527853446379526b3433646a6c36597a6c354d32553564334a53616e5655623256614e6d56575658426a4d574e6d4e7a683561324a6a4159436738634b784e437a5a47416b6d794e4e7566674a6f7753494737722d4c367442636534326f59716f485371776f494176522d567a64724e45536274734c454e37415f464b68396e447177394f73754d6b674f775a6a37774c4e727773227d"),
    ("devices", "000000f17b2274223a2264657669636573222c226c697374223a224f4752705a4470725a586b36656a5a4e6132357559315a745a5752776147644555324a5262316c4d65553568526c4a475a45497956335235616e686a526d524c566a5a3656475a6a57585a7a674b44787772453041514543417751464267634943516f4c4441304f4478415245684d554652595847426b6147787764486838674141415066553257664f6b4b6a59374171625a4d5753324f537a495271664233335053723361616f42692d704964396838314f715046724b447a77624a444e6c594975564554797878626176375234494a75444c794c554c227d"),
    ("accept", "000000a47b2274223a22616363657074222c2272656e657765645f6772616e74223a6e756c6c2c22766964656f223a7b226d61785f706978656c73223a3932313630302c226d61785f667073223a33302c226465636f6465223a5b22617631222c22767038225d2c22656e636f6465223a5b22767038222c22617631225d2c2268775f6465636f6465223a5b22767038225d2c2268775f656e636f6465223a5b22767038225d7d7d"),
    ("decline", "000000207b2274223a226465636c696e65222c22726561736f6e223a226c61746572227d"),
    ("cancel", "0000002c7b2274223a2263616e63656c222c22726561736f6e223a22616e7377657265645f656c73657768657265227d"),
    ("tokens", "000000ef7b2274223a22746f6b656e73222c226772616e7473223a5b224f4752705a4470725a586b36656a5a4e6132357559315a745a5752776147644555324a5262316c4d65553568526c4a475a45497956335235616e686a526d524c566a5a3656475a6a57585a7a41514944424155474277674a4367734d4451345045424553457851564668635947526f624842306548794341704b66614268724c58567a666c565070456f42724a354367754752535a3372424b6936394f6f393566534f684e684c42524d565a306d3139354366522d6c74714b67554e33327679656f5f5a3574356a5277436870417130774173225d7d"),
    ("group_letter", "000001ea7b2274223a2267726f75705f6c6574746572222c226174746573746174696f6e223a224f4752705a4470725a586b36656a5a4e6132357559315a745a5752776147644555324a5262316c4d65553568526c4a475a45497956335235616e686a526d524c566a5a3656475a6a57585a7a41514944424155474277674a4367734d4451345045424553457851564668635947526f624842306548794341704b66614268724c58567a666c565070456f42724a354367754752535a3372424b6936394f6f393566534f684e684c42524d565a306d3139354366522d6c74714b67554e33327679656f5f5a3574356a5277436870417130774173222c226772616e74223a224f4752705a4470725a586b36656a5a4e6132357559315a745a5752776147644555324a5262316c4d65553568526c4a475a45497956335235616e686a526d524c566a5a3656475a6a57585a7a41674d454251594843416b4b4377774e4467385145524954464255574678675a476873634852346649434741704b6661427570524c466969616a775a57566c59705f7178655252764877484878716961755a48704643664e4b317779593235743641775a38787570354655546b3878703464354c57612d6a416c6d516b694862726a4555337751222c226c6574746572223a226247563064475679227d"),
    ("gcall_heartbeat", "0000001b7b2274223a22686561727462656174222c2265706f6368223a377d"),
    ("gcall_active", "000000267b2274223a22616374697665222c2267656e223a322c22736c6f7473223a5b312c342c355d7d"),
];
const LINKS: &[(&str, &str)] = &[
    ("qr_none", "OSVL1:AEBQIBIGA4EASCQLBQGQ4DYQCEJBGFAVCYLRQGI2DMOB2HQ7EAQSECIJBEEQSCIJBEEQSCIJBEEQSCIAAPUAA"),
    ("qr_custom", "OSVL1:AEBQIBIGA4EASCQLBQGQ4DYQCEJBGFAVCYLRQGI2DMOB2HQ7EAQSECIJBEEQSCIJBEEQSCIJBEEQSCIAAPUOAGTIOR2HA4Z2F4XXEZLMMF4S4ZLYMFWXA3DFFZ2GK43U"),
    ("proof_scanner", "11046949ff66d752e6814745bb9e1578d7414cc4226edcbc66835a83a1ea1877"),
    ("proof_displayer", "e67927f17c0045de349227e067e7c77128eaf4eefa0ce9db37d7e962c119d316"),
    ("confirm_code", "623 727"),
];
const RANDOM_TOKEN: &str = "386469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a54666359767300386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b626380a0f1c2b13480fdccc4b1347ccb1d5f184382891ff64b86cede6025006d60113efb49a0e046e8914144a956ac69b239a757686676945ac06feb44fe13768face7d3a6ab109f86228747b003106311a79f156eaf9965894c366fd6870a";
const RANDOM_TICKET: &str = "OSVC2:PPJKPPDRZ3WCT3WAROFVBZTIR2T7OCJJECUQCMQ3TTSVWNVGKJUACAQDAQCQMBYIBEFAWDANBYHRAEISCMKBKFQXDAMRUGY4DUPB6IEC6MBUXC2BZKVSCBSHN5WGIZLOAINGQ5DUOBZTULZPOJSWYYLZFZSXQYLNOBWGKLTUMVZXJ4LYT2HCO4MLAZVYM3H2NO6L6YKJNKWKNKGA5F7NWVK4SUSSYRJWMY4FXFKHJ6YLDFBIWDSLMV6F57ZTLS5PRW47J6C2WLUSOUJWAYFA";
const RANDOM_LINK_GRANT: &str = "2dilswE33IGyp7G5Unlgg-OB2fzbkkCH6QUD9gNfBCvmqyJJeghVpm1HUKJs";
const RANDOM_GCALL_JOIN: &str = "0a6772702d676f6c64656e1641514944424155474277674a4367734d44513450454101386469643a6b65793a7a364d6b766d413471483732464e3776397a6339793365397772526a75546f655a3665565570633163663738796b626380a0f1c2b134818082d88fdabee2734a57ed34b2504e4e57eb50d99422fd51804936900bc9d142e83252e4f1515ccf64b87a1e166d74bb2cf5b8cdba30e27951bc2a5cad52e872fd0ece95dc579b0d";

#[test]
#[ignore = "generator"]
fn dump() {
    println!("const BLOBS: &[(&str, &str)] = &[");
    for (n, b) in blobs() {
        println!("    (\"{n}\", \"{}\"),", hex(&b));
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
    let g = issue_token(&a(), b().did(), Cap::Message, T0 * 1000, 3_600_000);
    println!("const RANDOM_TOKEN: &str = \"{}\";", hex(&g.to_bytes()));
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
    let (_, j) = gcall::sign_join(&b(), GID, CALL_ID, 1, T0 * 1000).unwrap();
    println!(
        "const RANDOM_GCALL_JOIN: &str = \"{}\";",
        hex(&j.to_bytes())
    );
}

#[test]
fn deterministic_blobs_are_byte_identical() {
    let built = blobs();
    assert_eq!(built.len(), BLOBS.len());
    for ((name, bytes), (gname, golden)) in built.iter().zip(BLOBS) {
        assert_eq!(name, gname);
        assert_eq!(&hex(bytes), golden, "{name} envelope bytes");
    }
}

fn captured<T: Signable>(name: &str) -> Signed<T> {
    let (_, h) = BLOBS.iter().find(|(n, _)| *n == name).unwrap();
    Signed::from_bytes(&unhex(h)).unwrap()
}

#[test]
fn captured_blobs_open() {
    let (a, b) = (a(), b());
    assert_eq!(
        verify_attestation(&captured("attestation")).unwrap().device,
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
    assert_eq!(
        captured::<gcall::CallStart>("gcall_start")
            .open()
            .unwrap()
            .k,
        3
    );
    // The roster's entry carries a real Join signature, so the whole roster opens.
    let roster = captured::<gcall::Roster>("gcall_roster").open().unwrap();
    assert_eq!(roster.entries.len(), 1);
    captured::<gcall::Leave>("gcall_leave").open().unwrap();
    assert_eq!(
        captured::<gcall::End>("gcall_end").open().unwrap().reason,
        gcall::EndReason::HostLeaving
    );
    captured::<gcall::HostRequest>("gcall_host_request")
        .open()
        .unwrap();
    captured::<gcall::HostRefusal>("gcall_host_refusal")
        .open()
        .unwrap();
    assert_eq!(
        captured::<gcall::GroupNote>("gcall_note")
            .open()
            .unwrap()
            .dev,
        dev(7)
    );
    let st = gcall_status::open_status_for(&captured("gcall_status"), GID, T0 * 1000 + 1).unwrap();
    assert_eq!(st.load, 10);
    profile::verify_picture_deletion(&captured("picture_deletion"), a.did()).unwrap();
    let g = captured::<Grant>("grant").redeem(&Expect { iss: Some(a.did()), holder: b.did(), now_ms: T0 * 1000 }, &Unrecorded).unwrap();
    assert_eq!(g.claim, Cap::Call);
    // A signature from another envelope is refused.
    let other = captured::<Attestation>("attestation");
    let list = captured::<DeviceList>("device_list");
    let bad = Signed::<Attestation>::from_parts(other.body().to_vec(), *list.sig());
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
    let tok = Signed::<Grant>::from_bytes(&unhex(RANDOM_TOKEN)).unwrap();
    let (m0, none) = (T0 * 1000, &Unrecorded);
    let g = verify_token(&tok, a.did(), b.did(), Cap::Message, m0 + 10, none).unwrap();
    assert_eq!((g.iat_ms, g.exp_ms), (m0, m0 + 3_600_000));
    assert_eq!(verify_token(&tok, a.did(), b.did(), Cap::Call, m0 + 10, none), Err(Error::BadCapability));
    assert_eq!(verify_token(&tok, a.did(), b.did(), Cap::Message, m0 + 3_600_000, none), Err(Error::Expired));

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

    let join = Signed::<gcall::Join>::from_bytes(&unhex(RANDOM_GCALL_JOIN)).unwrap();
    assert_eq!(join.open().unwrap().did, b.did());
}
