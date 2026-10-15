//! Golden vectors for `identity`, frozen BEFORE the dependency upgrades (bip39 3, ed25519/x25519-dalek 3,
//! hkdf 0.13, sha2 0.11, rand 0.10, argon2 0.6, aes-gcm 0.11, bincode maybe). Must pass UNCHANGED after.
//!
//! Produced by running `cargo test -p identity --test golden_identity -- --ignored --nocapture dump`
//! once on base 0e1eecb (bip39 2, bs58 0.5, bincode 1) and pasting the output. Phrase is the public
//! BIP-39 test phrase (not a real identity). Do NOT regenerate after an upgrade.

use identity::{
    Keystore, did_from_public_key, encrypt_for, public_key_from_did, recover, unlock, verify,
};

const PHRASE: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PASS: &str = "golden passphrase";
const MSG: &[u8] = b"tinline golden message";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

// ---- captured constants ----
const DID: &str = "did:key:z6MknncVmedphgDSbQoYLyNaFRFdB2WtyjxcFdKV6zTfcYvs";
const SIGNING_PUB: &str = "7bd2a7bc71ceec29eec08b8b50e6688ea7f7092920a901321b9ce55b36a65268";
const ENC_PUB: &str = "d9aa528f5ab4f3c62d656343f6d902ecb1e9e71b39f68ee2947735c51254af67";
const DEVICE_PUB: &str = "0c0618436db1e118daed795c37432dec64af063dda6a7bb3f0711b6fc4000273";
const SIG: &str = "cab272ce948bb9f654c7ce275ef27fd3e23a9627bed6c20ee362e41cc8b1fb3e39428e72369a4ecda7ccc5d647db2ead7ff713493316ecf4b08d5c735998750a";
const SEALED: &str = "a9ae485190966911966040ade83b1964a83ba9b0690d6d9fe7f0c2e487a3df1e74c8f0829d1ebd637aed6a93d2bc2fc8acf4d3e8473022d21f86e513adec3497e8975c6d25cf9c851563c5a6d259535871c4";
const KEYSTORE: &str = "0138000000000000006469643a6b65793a7a364d6b6e6e63566d6564706867445362516f594c794e614652466442325774796a786346644b56367a5466635976737bd2a7bc71ceec29eec08b8b50e6688ea7f7092920a901321b9ce55b36a65268d9aa528f5ab4f3c62d656343f6d902ecb1e9e71b39f68ee2947735c51254af670c0618436db1e118daed795c37432dec64af063dda6a7bb3f0711b6fc40002733c00000000000000938cdbcbdccd5003d7cbb9d3fb4efd61b0396b0d31053b4468417f2ed8839657e17253313d864430aae19cbdbf21266d0335f419ba233c80ce382a303c0000000000000044c34b2f8d525a29f512c48b621532e404172c10cd99f9bf2548d97f4ecff0244343fc01c4c383a5b94200e72d5eef2d36223e24237918240b8d0a113c00000000000000a0337927f390ce74f46221df69f38745c594b20adbf6410d10cb4f59d2c0803cc8f06fe0857f5cc212036bcc8dc82d122961660a04fdbcc1e13ea69e0fd7dc051dff62115020d7b3906cb0c5000001000300000004000000";

#[test]
#[ignore = "generator"]
fn dump() {
    let id = recover(PHRASE).unwrap();
    println!("const DID: &str = \"{}\";", id.did());
    println!(
        "const SIGNING_PUB: &str = \"{}\";",
        hex(&id.signing_public_key())
    );
    println!(
        "const ENC_PUB: &str = \"{}\";",
        hex(&id.encryption_public_key())
    );
    println!(
        "const DEVICE_PUB: &str = \"{}\";",
        hex(&id.device_public_key())
    );
    println!("const SIG: &str = \"{}\";", hex(&id.sign(MSG)));
    println!(
        "const SEALED: &str = \"{}\";",
        hex(&encrypt_for(&id.encryption_public_key(), MSG).unwrap())
    );
    println!(
        "const KEYSTORE: &str = \"{}\";",
        hex(&identity::seal(&id, PASS).unwrap().to_bytes())
    );
}

#[test]
fn phrase_to_keys_and_did() {
    let id = recover(PHRASE).unwrap();
    assert_eq!(id.did(), DID);
    assert_eq!(hex(&id.signing_public_key()), SIGNING_PUB);
    assert_eq!(hex(&id.encryption_public_key()), ENC_PUB);
    assert_eq!(hex(&id.device_public_key()), DEVICE_PUB);
    // Ed25519 signatures are deterministic.
    assert_eq!(hex(&id.sign(MSG)), SIG);
    assert!(verify(
        &id.signing_public_key(),
        MSG,
        &unhex(SIG).try_into().unwrap()
    ));
}

#[test]
fn phrase_normalisation_is_unchanged() {
    // Whatever bip39 accepts today as extra whitespace must keep giving the same key.
    let spaced = PHRASE.replace(' ', "  ");
    if let Ok(id) = recover(&spaced) {
        assert_eq!(id.did(), DID);
    }
    assert!(recover("abandon abandon").is_err());
    assert!(
        recover(&PHRASE.replace("about", "abandon")).is_err(),
        "bad checksum"
    );
}

#[test]
fn did_encoding() {
    assert!(DID.starts_with("did:key:z6Mk"));
    let public: [u8; 32] = unhex(SIGNING_PUB).try_into().unwrap();
    assert_eq!(did_from_public_key(&public), DID);
    assert_eq!(public_key_from_did(DID), Some(public));
    assert_eq!(public_key_from_did("did:key:z"), None);
    assert_eq!(public_key_from_did("did:web:example"), None);
    assert_eq!(public_key_from_did(&format!("{DID}1")), None);
}

#[test]
fn captured_ecies_ciphertext_opens() {
    let id = recover(PHRASE).unwrap();
    assert_eq!(id.decrypt_sealed(&unhex(SEALED)).unwrap(), MSG);
}

#[test]
fn captured_keystore_bytes_unlock() {
    // The bincode layout of the keystore file (version, did, 3 pubkeys, 3 sealed secrets, salt, m/t/p).
    let bytes = unhex(KEYSTORE);
    let ks = Keystore::from_bytes(&bytes).unwrap();
    assert_eq!(ks.did(), DID);
    assert_eq!(
        ks.to_bytes(),
        bytes,
        "re-serialising must give the same bytes"
    );
    let id = unlock(&ks, PASS).unwrap();
    assert_eq!(id.did(), DID);
    assert_eq!(hex(&id.device_public_key()), DEVICE_PUB);
    assert_eq!(hex(&id.sign(MSG)), SIG);
    assert!(matches!(
        unlock(&ks, "wrong"),
        Err(identity::IdentityError::WrongPassphrase)
    ));
    // Layout facts: version byte first, then the DID length as u64 LE.
    assert_eq!(bytes[0], 1);
    assert_eq!(
        u64::from_le_bytes(bytes[1..9].try_into().unwrap()) as usize,
        DID.len()
    );
}
