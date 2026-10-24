use serde_json::Value;

use crate::api::{RegisterRequest, TagRequest, WakeRequest};
use crate::push::{Auth, Outcome, PushConfig};
use crate::{
    DAY_MS, Hint, Kind, Kinds, RelayKey, Target, WakeClaim, WakeError, hint, jwt, push, relay,
    token,
};

const NOW: u64 = 1_790_000_000_000;

fn key(kid: u8) -> RelayKey {
    RelayKey {
        kid,
        key: [kid.wrapping_add(7); 32],
    }
}

fn cfg() -> PushConfig {
    PushConfig {
        fcm_project: "tinline".into(),
        apns_topic: "com.osvauld.tinline".into(),
    }
}

/// Distinct nonces, as the Worker's random source would give.
fn nonces() -> impl FnMut() -> [u8; 12] {
    let mut n = 0u8;
    move || {
        n += 1;
        [n; 12]
    }
}

fn fcm() -> Target {
    Target::Fcm {
        token: "fcm-token_abcdefghij:0123456789".into(),
    }
}

fn apns(voip: bool) -> Target {
    Target::Apns {
        alert: "ab".repeat(32),
        voip: voip.then(|| "cd".repeat(32)),
        sandbox: false,
    }
}

fn up() -> Target {
    Target::UnifiedPush {
        endpoint: "https://ntfy.example.org/upAbCdEf?up=1".into(),
    }
}

fn claim(target: Target, kinds: Kinds) -> WakeClaim {
    WakeClaim {
        target,
        hint_key: [9; 32],
        tag: [5; 16],
        kinds,
        exp_ms: NOW + 30 * DAY_MS,
    }
}

fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("json body")
}

fn header<'a>(req: &'a push::HttpRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn token_round_trip_and_tamper() {
    let c = claim(apns(true), Kinds::ALL);
    let t = token::seal(&key(1), &c, [3; 12]).unwrap();
    assert!(t.len() < 600, "token is {} chars", t.len());
    assert_eq!(token::open(&[key(1)], &t).unwrap(), c);

    // A rotated relay still opens tokens of a key it keeps; a retired key gives UnknownKey.
    assert_eq!(token::open(&[key(2), key(1)], &t).unwrap(), c);
    assert_eq!(token::open(&[key(2)], &t), Err(WakeError::UnknownKey));

    let mut raw = crate::b64::decode(&t).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    assert_eq!(
        token::open(&[key(1)], &crate::b64::encode(&raw)),
        Err(WakeError::Sealed)
    );
    assert_eq!(token::open(&[key(1)], "!!"), Err(WakeError::Malformed));
    assert_eq!(
        token::open(&[key(1)], &"A".repeat(5000)),
        Err(WakeError::TooLong)
    );
}

#[test]
fn token_does_not_show_its_contents() {
    let c = claim(fcm(), Kinds::MSG);
    let t = token::seal(&key(1), &c, [3; 12]).unwrap();
    assert!(!t.contains("fcm-token"));
    // Same claim, different nonce: unlinkable tokens.
    assert_ne!(t, token::seal(&key(1), &c, [4; 12]).unwrap());
}

#[test]
fn claim_check() {
    let c = claim(apns(false), Kinds::ALL);
    assert_eq!(c.check(Kind::Msg, NOW), Ok(()));
    // No VoIP token: never a call wake, whatever the kinds say.
    assert_eq!(c.check(Kind::Call, NOW), Err(WakeError::KindNotAllowed));
    let msg_only = claim(fcm(), Kinds::MSG);
    assert_eq!(
        msg_only.check(Kind::Call, NOW),
        Err(WakeError::KindNotAllowed)
    );
    assert_eq!(
        msg_only.check(Kind::Msg, msg_only.exp_ms),
        Err(WakeError::Expired)
    );
}

#[test]
fn kinds() {
    assert!(Kinds::ALL.allows(Kind::Msg) && Kinds::ALL.allows(Kind::Call));
    assert!(!Kinds::MSG.allows(Kind::Call));
    assert!(!Kinds::default().is_valid());
    let bad: Kinds = serde_json::from_str("4").unwrap();
    assert!(!bad.is_valid());
}

#[test]
fn target_validation() {
    assert!(fcm().validate().is_ok());
    assert!(apns(true).validate().is_ok());
    assert!(up().validate().is_ok());
    let bad = [
        Target::Fcm {
            token: "short".into(),
        },
        Target::Fcm {
            token: "has space in it 0123456789".into(),
        },
        Target::Apns {
            alert: "zz".repeat(32),
            voip: None,
            sandbox: false,
        },
        Target::Apns {
            alert: "ab".repeat(32),
            voip: Some("x".into()),
            sandbox: false,
        },
        Target::UnifiedPush {
            endpoint: "http://ntfy.example.org/x".into(),
        },
        Target::UnifiedPush {
            endpoint: "https://10.0.0.1/x".into(),
        },
        Target::UnifiedPush {
            endpoint: "https://ntfy.example.org:8443/x".into(),
        },
        Target::UnifiedPush {
            endpoint: "https://user@ntfy.example.org/x".into(),
        },
        Target::UnifiedPush {
            endpoint: "https://[::1]/x".into(),
        },
        Target::UnifiedPush {
            endpoint: "https://localhost/x".into(),
        },
        Target::UnifiedPush {
            endpoint: "https://ntfy.example.org/a b".into(),
        },
    ];
    for t in bad {
        assert!(
            matches!(t.validate(), Err(WakeError::BadTarget(_))),
            "{t:?}"
        );
    }
}

#[test]
fn hint_round_trip_and_unlinkable() {
    let h = Hint {
        tag: [5; 16],
        kind: Kind::Call,
        sent_at_ms: NOW,
    };
    let a = hint::seal(&[9; 32], &h, [1; 12]).unwrap();
    let b = hint::seal(&[9; 32], &h, [2; 12]).unwrap();
    assert_ne!(a, b);
    assert!(a.len() < 120, "hint is {} chars", a.len());
    assert_eq!(hint::open(&[9; 32], &a).unwrap(), h);
    assert_eq!(hint::open(&[8; 32], &a), Err(WakeError::Sealed));
}

#[test]
fn stale_call_hint() {
    let call = Hint {
        tag: [0; 16],
        kind: Kind::Call,
        sent_at_ms: NOW,
    };
    assert!(!call.is_stale_call(NOW + hint::STALE_CALL_MS));
    assert!(call.is_stale_call(NOW + hint::STALE_CALL_MS + 1));
    let msg = Hint {
        kind: Kind::Msg,
        ..call
    };
    assert!(!msg.is_stale_call(NOW + DAY_MS));
}

fn register_req(target: Target, kinds: Kinds) -> RegisterRequest {
    RegisterRequest {
        target,
        hint_key: [9; 32],
        tags: vec![
            TagRequest {
                tag: [1; 16],
                kinds,
                exp_ms: NOW + 30 * DAY_MS,
            },
            TagRequest {
                tag: [2; 16],
                kinds: Kinds::MSG,
                exp_ms: NOW + DAY_MS,
            },
        ],
    }
}

#[test]
fn register_then_wake() {
    let resp = relay::register(
        &key(1),
        &register_req(apns(true), Kinds::ALL),
        NOW,
        nonces(),
    )
    .unwrap();
    assert_eq!(resp.tokens.len(), 2);
    assert_eq!(resp.tokens[0].tag, [1; 16]);

    let wake = WakeRequest {
        token: resp.tokens[0].token.clone(),
        kind: Kind::Call,
    };
    let c = relay::open_for_wake(&[key(1)], &wake, NOW).unwrap();
    assert_eq!(c.tag, [1; 16]);
    let req = relay::wake_request(&cfg(), &c, Kind::Call, NOW, [7; 12]).unwrap();
    let sent = body_json(&req.body)["h"].as_str().unwrap().to_owned();
    let opened = hint::open(&[9; 32], &sent).unwrap();
    assert_eq!(
        opened,
        Hint {
            tag: [1; 16],
            kind: Kind::Call,
            sent_at_ms: NOW
        }
    );

    // The second contact may only send message wakes.
    let wake = WakeRequest {
        token: resp.tokens[1].token.clone(),
        kind: Kind::Call,
    };
    assert_eq!(
        relay::open_for_wake(&[key(1)], &wake, NOW),
        Err(WakeError::KindNotAllowed)
    );
}

#[test]
fn register_refusals() {
    let mut r = register_req(fcm(), Kinds::ALL);
    r.tags[0].exp_ms = NOW + 40 * DAY_MS;
    assert_eq!(
        relay::register(&key(1), &r, NOW, nonces()),
        Err(WakeError::BadRequest("expiry"))
    );
    let mut r = register_req(fcm(), Kinds::ALL);
    r.tags[0].exp_ms = NOW;
    assert_eq!(
        relay::register(&key(1), &r, NOW, nonces()),
        Err(WakeError::BadRequest("expiry"))
    );
    let mut r = register_req(fcm(), Kinds::ALL);
    r.tags.clear();
    assert!(relay::register(&key(1), &r, NOW, nonces()).is_err());
    let r = register_req(apns(false), Kinds::ALL);
    assert_eq!(
        relay::register(&key(1), &r, NOW, nonces()),
        Err(WakeError::BadRequest("call wakes need a voip token"))
    );
    let r = register_req(Target::Fcm { token: "x".into() }, Kinds::MSG);
    assert!(matches!(
        relay::register(&key(1), &r, NOW, nonces()),
        Err(WakeError::BadTarget(_))
    ));
}

#[test]
fn register_json_shape() {
    let text = r#"{"target":{"fcm":{"token":"fcm-token_abcdefghij:0123456789"}},
        "hint_key":"CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk",
        "tags":[{"tag":"AQEBAQEBAQEBAQEBAQEBAQ","kinds":3,"exp_ms":1790000100000}]}"#;
    let r: RegisterRequest = serde_json::from_str(text).unwrap();
    assert_eq!(r.hint_key, [9; 32]);
    assert_eq!(r.tags[0].tag, [1; 16]);
    let w: WakeRequest = serde_json::from_str(r#"{"token":"x","kind":"call"}"#).unwrap();
    assert_eq!(w.kind, Kind::Call);
}

#[test]
fn fcm_requests() {
    let msg = push::request(&cfg(), &fcm(), Kind::Msg, "HINT", NOW).unwrap();
    assert_eq!(
        msg.url,
        "https://fcm.googleapis.com/v1/projects/tinline/messages:send"
    );
    assert_eq!(msg.auth, Auth::Fcm);
    let b = body_json(&msg.body);
    assert_eq!(b["message"]["token"], "fcm-token_abcdefghij:0123456789");
    assert_eq!(b["message"]["data"], serde_json::json!({ "h": "HINT" }));
    assert_eq!(b["message"]["android"]["priority"], "HIGH");
    assert_eq!(b["message"]["android"]["collapse_key"], "wake-msg");
    assert!(
        b["message"].get("notification").is_none(),
        "data-only: the app decides what to show"
    );

    let call = body_json(
        &push::request(&cfg(), &fcm(), Kind::Call, "HINT", NOW)
            .unwrap()
            .body,
    );
    assert_eq!(call["message"]["android"]["ttl"], "30s");
    assert!(call["message"]["android"].get("collapse_key").is_none());
}

#[test]
fn apns_requests() {
    let call = push::request(&cfg(), &apns(true), Kind::Call, "HINT", NOW).unwrap();
    assert_eq!(
        call.url,
        format!("https://api.push.apple.com/3/device/{}", "cd".repeat(32))
    );
    assert_eq!(header(&call, "apns-push-type"), Some("voip"));
    assert_eq!(
        header(&call, "apns-topic"),
        Some("com.osvauld.tinline.voip")
    );
    assert_eq!(header(&call, "apns-expiration"), Some("0"));
    assert_eq!(body_json(&call.body), serde_json::json!({ "h": "HINT" }));
    assert_eq!(call.auth, Auth::Apns);

    let msg = push::request(&cfg(), &apns(true), Kind::Msg, "HINT", NOW).unwrap();
    assert!(msg.url.ends_with(&"ab".repeat(32)));
    assert_eq!(header(&msg, "apns-push-type"), Some("alert"));
    assert_eq!(header(&msg, "apns-topic"), Some("com.osvauld.tinline"));
    assert_eq!(
        header(&msg, "apns-expiration"),
        Some((NOW / 1000 + 3600).to_string().as_str())
    );
    let b = body_json(&msg.body);
    assert_eq!(b["aps"]["alert"]["body"], push::APNS_MSG_TEXT);
    assert_eq!(b["aps"]["content-available"], 1);

    assert!(push::request(&cfg(), &apns(false), Kind::Call, "HINT", NOW).is_none());
    let sandbox = Target::Apns {
        alert: "ab".repeat(32),
        voip: None,
        sandbox: true,
    };
    let req = push::request(&cfg(), &sandbox, Kind::Msg, "HINT", NOW).unwrap();
    assert!(
        req.url
            .starts_with("https://api.sandbox.push.apple.com/3/device/")
    );
}

#[test]
fn unifiedpush_request() {
    let req = push::request(&cfg(), &up(), Kind::Call, "HINT", NOW).unwrap();
    assert_eq!(req.url, "https://ntfy.example.org/upAbCdEf?up=1");
    assert_eq!(req.body, b"HINT");
    assert_eq!(header(&req, "urgency"), Some("high"));
    assert_eq!(header(&req, "ttl"), Some("30"));
    assert_eq!(req.auth, Auth::None);
}

#[test]
fn platform_outcomes() {
    assert_eq!(push::outcome(&fcm(), 200, ""), Outcome::Sent);
    assert_eq!(
        push::outcome(&fcm(), 404, r#"{"error":{"status":"NOT_FOUND"}}"#),
        Outcome::Gone
    );
    assert_eq!(push::outcome(&fcm(), 401, ""), Outcome::AuthFailed);
    assert_eq!(push::outcome(&fcm(), 503, ""), Outcome::Retry);
    assert_eq!(
        push::outcome(&apns(true), 410, r#"{"reason":"Unregistered"}"#),
        Outcome::Gone
    );
    assert_eq!(
        push::outcome(&apns(true), 400, r#"{"reason":"BadDeviceToken"}"#),
        Outcome::Gone
    );
    assert_eq!(
        push::outcome(&apns(true), 403, r#"{"reason":"ExpiredProviderToken"}"#),
        Outcome::AuthFailed
    );
    assert_eq!(
        push::outcome(&apns(true), 400, r#"{"reason":"BadTopic"}"#),
        Outcome::Rejected
    );
    assert_eq!(push::outcome(&apns(true), 429, ""), Outcome::Retry);
    assert_eq!(push::outcome(&up(), 410, ""), Outcome::Gone);
}

#[test]
fn error_statuses() {
    assert_eq!(WakeError::Expired.status(), 410);
    assert_eq!(WakeError::UnknownKey.status(), 410);
    assert_eq!(WakeError::KindNotAllowed.status(), 403);
    assert_eq!(WakeError::Sealed.status(), 400);
}

#[test]
fn keys_for_storage() {
    let c = claim(fcm(), Kinds::ALL);
    assert_eq!(relay::revoke_key(&c), format!("rv:{}", "05".repeat(16)));
    assert_eq!(
        relay::rate_key(&c, Kind::Call),
        format!("{}:call", "05".repeat(16))
    );
}

#[test]
fn apns_jwt_verifies_as_es256() {
    use p256::ecdsa::signature::{Signer, Verifier};
    use p256::ecdsa::{Signature, SigningKey};

    let input = jwt::apns_input("TEAM123456", "KEY1234567", 1_790_000_000);
    let (h, c) = input.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&crate::b64::decode(h).unwrap()).unwrap();
    let claims: Value = serde_json::from_slice(&crate::b64::decode(c).unwrap()).unwrap();
    assert_eq!(
        header,
        serde_json::json!({ "alg": "ES256", "kid": "KEY1234567" })
    );
    assert_eq!(
        claims,
        serde_json::json!({ "iss": "TEAM123456", "iat": 1_790_000_000u64 })
    );

    // WebCrypto's ECDSA output is raw r || s (64 bytes), the same as `Signature::to_bytes`.
    let sk = SigningKey::from_slice(&[0x42; 32]).unwrap();
    let sig: Signature = sk.sign(input.as_bytes());
    let token = jwt::finish(&input, &sig.to_bytes());
    let (signed, s) = token.rsplit_once('.').unwrap();
    assert_eq!(signed, input);
    let sig = Signature::from_slice(&crate::b64::decode(s).unwrap()).unwrap();
    assert!(sk.verifying_key().verify(signed.as_bytes(), &sig).is_ok());
}

#[test]
fn fcm_assertion_and_exchange() {
    let sa = jwt::ServiceAccount::from_json(
        r#"{"type":"service_account","project_id":"tinline","private_key_id":"abc",
            "private_key":"-----BEGIN PRIVATE KEY-----\nAAEC\nAwQ=\n-----END PRIVATE KEY-----\n",
            "client_email":"wake-relay@tinline.iam.gserviceaccount.com",
            "token_uri":"https://oauth2.googleapis.com/token"}"#,
    )
    .unwrap();
    assert!(
        !format!("{sa:?}").contains("PRIVATE"),
        "Debug must not print the key"
    );
    assert_eq!(
        jwt::pkcs8_der(&sa.private_key).unwrap(),
        vec![0, 1, 2, 3, 4]
    );

    let input = jwt::fcm_assertion_input(&sa, 1000);
    let claims: Value =
        serde_json::from_slice(&crate::b64::decode(input.split_once('.').unwrap().1).unwrap())
            .unwrap();
    assert_eq!(claims["scope"], jwt::FCM_SCOPE);
    assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
    assert_eq!(claims["exp"], 4600);

    let req = jwt::fcm_token_request(&sa, "a.b.c");
    assert_eq!(req.url, "https://oauth2.googleapis.com/token");
    assert!(
        String::from_utf8(req.body)
            .unwrap()
            .ends_with("&assertion=a.b.c")
    );
    assert_eq!(
        jwt::fcm_token_response(
            r#"{"access_token":"ya29.x","expires_in":3599,"token_type":"Bearer"}"#
        )
        .unwrap(),
        ("ya29.x".to_string(), 3599)
    );
    assert!(jwt::pkcs8_der("no pem here").is_err());
}
