//! Golden vectors for the `cryptography` wrappers, frozen BEFORE the planned dependency
//! upgrades (rand 0.10, ed25519/x25519-dalek 3, sha2 0.11, hkdf 0.13, aes-gcm 0.11, argon2 0.6).
//! After an upgrade these tests must pass UNCHANGED: that proves no key, stored byte or wire
//! byte moved.
//!
//! How the constants were produced: `cargo test -p cryptography --test golden_vectors -- --ignored
//! --nocapture dump_vectors` was run once on the pre-upgrade code (base 0e1eecb: aes-gcm 0.10,
//! hkdf 0.12, sha2 0.10, argon2 0.5, ed25519-dalek 2, x25519-dalek 2, rand 0.8) and its output pasted
//! below. All inputs are fixed synthetic bytes. Random-nonce operations (AEAD, ECIES seal) are
//! tested on the open side with a ciphertext captured by the same dump. Do NOT regenerate after
//! an upgrade; a mismatch is the finding.

use cryptography::{aead, ecies, kdf, signature};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn arr32(s: &str) -> [u8; 32] {
    unhex(s).try_into().unwrap()
}

/// 32 bytes `start, start+1, ...` (wrapping).
fn seq(start: u8) -> [u8; 32] {
    core::array::from_fn(|i| start.wrapping_add(i as u8))
}

const KEY: [u8; 32] = [0x11; 32];
const PLAIN: &[u8] = b"tinline golden plaintext \xe2\x9c\x93";
const MSG: &[u8] = b"tinline golden message";
const SALT16: &[u8; 16] = b"golden-salt-0016";

// ---- captured constants (see module comment) ----
const HKDF_NO_SALT: &str = "5db7dcdfce550f591c19544b1b574d9bb01165a0dc0d56307599709f8e5528a4";
const HKDF_SALT: &str = "d69a8e07f843de0b5192a997b8cda6137c6433bc3b4d7160c7876080140a1a82";
const HKDF_EMPTY: &str = "eb70f01dede9afafa449eee1b1286504e1f62388b3f7dd4f956697b0e828fe18";
const ARGON_REAL: &str = "31fcb7d8c725d73231b2eca1571b7e7734768d328cf8efcc0215023aaa1c9122";
const ARGON_LOW: &str = "27a1d3db987f30a7985a5fa5e30c08ab06fb829bd89db6c3983871410f341890";
const ED_PUB_SEQ0: &str = "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";
const ED_SIG: &str = "765b7aa78a9eb0b60d91aeba290f4ee6d646fd024401096f2116430a32857b7af2310c852c23d4ea09a71b89b372aff800c48643d2af654907bed91b0e5bad01";
const X_PUB_A: &str = "79a631eede1bf9c98f12032cdeadd0e7a079398fc786b88cc846ec89af85a51a";
const X_PUB_B: &str = "493e82fc74464a59268817623d2053c5eb8e2cc4a988b4fee179ec6b010d531d";
const X_SHARED: &str = "dcd77236231add34de0561c47859a65d304f2a8550e8df98df053cf5dfabea0b";
const AEAD_CT: &str = "badde05b826fa9de6a07f0144f9f8323f237a381865e6d39810f53b665e9ed2571954bdc840a30c2de1c9b6a621400929119637235ab33c1";
const ECIES_CT: &str = "206b1dde9e3a5bbf685a7fb40647d3a6e301cd4b78a8efaabde85f786f7aa9578bc7f6a748f750afbc22fe36c7c1f3143f363a417c34b97f14855c8a954524545b72eec0ed8b0adb8bf091aebbd5b4d0451323d7173779e4";

#[test]
#[ignore = "generator: prints the constants above"]
fn dump_vectors() {
    let p = |n: &str, v: &[u8]| println!("const {n}: &str = \"{}\";", hex(v));
    p(
        "HKDF_NO_SALT",
        &kdf::hkdf_sha256(&seq(0), None, b"osv/golden/info"),
    );
    p(
        "HKDF_SALT",
        &kdf::hkdf_sha256(&seq(0), Some(SALT16), b"osv/golden/info"),
    );
    p("HKDF_EMPTY", &kdf::hkdf_sha256(b"", None, b""));
    p(
        "ARGON_REAL",
        &kdf::argon2id(b"correct horse battery staple", SALT16, 65536, 3, 4).unwrap(),
    );
    p("ARGON_LOW", &kdf::argon2id(b"pw", SALT16, 8, 1, 1).unwrap());
    p("ED_PUB_SEQ0", &signature::public_key(&seq(0)));
    p("ED_SIG", &signature::sign(&seq(0), MSG));
    p("X_PUB_A", &ecies::public_key(&seq(0x40)));
    p("X_PUB_B", &ecies::public_key(&seq(0x80)));
    p(
        "X_SHARED",
        &ecies::ecdh(&seq(0x40), &ecies::public_key(&seq(0x80))),
    );
    p("AEAD_CT", &aead::encrypt(&KEY, PLAIN).unwrap());
    p(
        "ECIES_CT",
        &ecies::seal(&ecies::public_key(&seq(0x80)), PLAIN).unwrap(),
    );
}

#[test]
fn hkdf_sha256_vectors() {
    assert_eq!(
        hex(&kdf::hkdf_sha256(&seq(0), None, b"osv/golden/info")),
        HKDF_NO_SALT
    );
    assert_eq!(
        hex(&kdf::hkdf_sha256(&seq(0), Some(SALT16), b"osv/golden/info")),
        HKDF_SALT
    );
    assert_eq!(hex(&kdf::hkdf_sha256(b"", None, b"")), HKDF_EMPTY);
    // Some(&[]) salt is the same as None for HKDF (zero-filled salt of hash length).
    assert_eq!(
        kdf::hkdf_sha256(&seq(0), Some(&[]), b"osv/golden/info"),
        kdf::hkdf_sha256(&seq(0), None, b"osv/golden/info")
    );
}

#[test]
fn argon2id_vectors_real_vault_params() {
    // docs/vault.md: m=65536 KiB, t=3, p=4 (also the keystore's parameters).
    let k = kdf::argon2id(b"correct horse battery staple", SALT16, 65536, 3, 4).unwrap();
    assert_eq!(hex(&k), ARGON_REAL);
    assert_eq!(
        hex(&kdf::argon2id(b"pw", SALT16, 8, 1, 1).unwrap()),
        ARGON_LOW
    );
    // Invalid params and a too-short salt stay errors (the vault maps these to refusals).
    assert!(kdf::argon2id(b"pw", SALT16, 0, 1, 1).is_err());
    assert!(kdf::argon2id(b"pw", b"short", 8, 1, 1).is_err());
}

#[test]
fn ed25519_vectors() {
    let public = signature::public_key(&seq(0));
    assert_eq!(hex(&public), ED_PUB_SEQ0);
    let sig = signature::sign(&seq(0), MSG);
    // Ed25519 is deterministic: the signature bytes must not move.
    assert_eq!(hex(&sig), ED_SIG);
    assert!(signature::verify(&public, MSG, &sig));
    assert!(!signature::verify(&public, b"other", &sig));
    let mut bad = sig;
    bad[0] ^= 1;
    assert!(!signature::verify(&public, MSG, &bad));
    // A captured signature verifies against the captured public key, independent of signing.
    assert!(signature::verify(
        &arr32(ED_PUB_SEQ0),
        MSG,
        &unhex(ED_SIG).try_into().unwrap()
    ));
}

#[test]
fn x25519_vectors() {
    let (a, b) = (seq(0x40), seq(0x80));
    assert_eq!(hex(&ecies::public_key(&a)), X_PUB_A);
    assert_eq!(hex(&ecies::public_key(&b)), X_PUB_B);
    assert_eq!(hex(&ecies::ecdh(&a, &arr32(X_PUB_B))), X_SHARED);
    assert_eq!(hex(&ecies::ecdh(&b, &arr32(X_PUB_A))), X_SHARED);
}

#[test]
fn aead_captured_ciphertext_opens() {
    // Layout: nonce(12) || ciphertext || tag(16).
    let ct = unhex(AEAD_CT);
    assert_eq!(ct.len(), 12 + PLAIN.len() + 16);
    assert_eq!(aead::decrypt(&KEY, &ct).unwrap(), PLAIN);
    let mut bad = ct.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(aead::decrypt(&KEY, &bad).is_err());
    assert!(aead::decrypt(&[0x12; 32], &ct).is_err());
    // New output has the same layout and round-trips (nonce is random, so only shape is asserted).
    let fresh = aead::encrypt(&KEY, PLAIN).unwrap();
    assert_eq!(fresh.len(), ct.len());
    assert_eq!(aead::decrypt(&KEY, &fresh).unwrap(), PLAIN);
}

#[test]
fn ecies_captured_ciphertext_opens() {
    // Layout: ephemeral_public(32) || aead(nonce | ciphertext | tag).
    let ct = unhex(ECIES_CT);
    assert_eq!(ct.len(), 32 + 12 + PLAIN.len() + 16);
    assert_eq!(ecies::open(&seq(0x80), &ct).unwrap(), PLAIN);
    assert!(ecies::open(&seq(0x40), &ct).is_err());
    let fresh = ecies::seal(&arr32(X_PUB_B), PLAIN).unwrap();
    assert_eq!(fresh.len(), ct.len());
    assert_eq!(ecies::open(&seq(0x80), &fresh).unwrap(), PLAIN);
}
