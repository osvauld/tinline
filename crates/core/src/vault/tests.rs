use super::*;

#[test]
fn no_passphrase_slot_round_trip() {
    let secrets = Secrets { mnemonic: "m".into(), device_secret: [5; 32] };
    let (v, dek) = seal(&secrets, None).unwrap();
    assert!(!v.has_passphrase());
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("sealed") && !json.contains("salt") && !json.contains("wrapped_dek"));
    let v: Vault = serde_json::from_str(&json).unwrap();
    assert_eq!(open_with_key(&v, &dek[..]).unwrap().device_secret, [5; 32]);
    assert!(open(&v, "anything").is_err());
    assert!(!rewrap(&v, &dek, "later").unwrap().wrapped_dek.is_empty());
}

#[test]
fn parameter_bounds() {
    // Rejected before any hashing happens.
    for (m, t, p, s) in [
        (1024, 3, 4, 16),
        (1 << 20, 3, 4, 16),
        (65536, 1, 4, 16),
        (65536, 11, 4, 16),
        (65536, 3, 0, 16),
        (65536, 3, 9, 16),
        (65536, 3, 4, 8),
        (65536, 3, 4, 65),
    ] {
        assert!(kek("pass", &vec![7u8; s], kdf::ArgonParams { m, t, p }).is_err(), "{m} {t} {p} {s}");
    }
    assert!(kek("pass", &[7u8; 16], kdf::ArgonParams { m: 65536, t: 3, p: 4 }).is_ok());
    assert!(kek("pass", &[7u8; 64], kdf::ArgonParams { m: 65536, t: 10, p: 8 }).is_ok());
}
