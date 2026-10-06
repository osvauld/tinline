use super::*;

#[test]
fn generate_then_unlock_yields_same_did() {
    let (created, _mnemonic) = generate();
    let keystore = seal(&created, "pw").unwrap();
    let unlocked = unlock(&keystore, "pw").unwrap();
    assert_eq!(created.did(), unlocked.did());
}

#[test]
fn recover_reproduces_did_from_mnemonic() {
    let (created, mnemonic) = generate();
    let recovered = recover(&mnemonic.to_string()).unwrap();
    assert_eq!(created.did(), recovered.did());
}

#[test]
fn recover_rejects_bad_mnemonic() {
    assert!(matches!(
        recover("not a valid mnemonic phrase"),
        Err(IdentityError::Bip39)
    ));
}
