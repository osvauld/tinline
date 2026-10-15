use super::*;

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
