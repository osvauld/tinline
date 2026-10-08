use p2pcore::{Error, accounts::Accounts};
use std::path::PathBuf;

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("tinline-accounts-{}", rand::random::<u128>()));
        Self(dir)
    }
    fn db(&self, did: &str) -> PathBuf {
        self.0
            .join(format!("{}.redb", did.strip_prefix("did:key:").unwrap()))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn restore_same_did_distinct_devices_and_sealed_labels() {
    let phone_dir = Dir::new();
    let desktop_dir = Dir::new();
    let phone = Accounts::open(&phone_dir.0).unwrap();
    let desktop = Accounts::open(&desktop_dir.0).unwrap();
    let (p, phrase) = phone
        .create("secret display label", "private phone label", "phone pass")
        .unwrap();
    let d = desktop
        .import(
            &phrase,
            "secret display label",
            "private desktop label",
            "desktop pass",
        )
        .unwrap();
    assert_eq!(p.did, d.did);
    assert_ne!(
        phone.current().unwrap().device_public,
        desktop.current().unwrap().device_public
    );
    phone
        .put(&p, "contacts/main", b"sensitive contact data")
        .unwrap();
    phone.lock();
    let raw = std::fs::read(phone_dir.db(&p.did)).unwrap();
    for secret in [
        phrase.as_bytes(),
        b"secret display label",
        b"private phone label",
        b"sensitive contact data",
    ] {
        assert!(!raw.windows(secret.len()).any(|w| w == secret));
    }
    assert!(matches!(
        phone.unlock(&p.did, "wrong"),
        Err(Error::WrongPassphrase)
    ));
    let restored = phone.unlock(&p.did, "phone pass").unwrap();
    assert_eq!(
        &**phone
            .get(&restored, "contacts/main")
            .unwrap()
            .as_ref()
            .unwrap(),
        b"sensitive contact data"
    );
    assert_eq!(phone.current().unwrap().device_label, "private phone label");
    assert!(desktop.get(&d, "contacts/main").unwrap().is_none());
}

#[test]
fn accounts_are_isolated_and_old_sessions_stay_invalid_after_switch_back() {
    let dir = Dir::new();
    let accounts = Accounts::open(&dir.0).unwrap();
    let (a, _) = accounts.create("a", "phone", "pass").unwrap();
    accounts.put(&a, "saved/you", b"a only").unwrap();
    let (b, _) = accounts.create("b", "phone", "pass").unwrap();
    assert_eq!(accounts.accounts().unwrap().len(), 2);
    assert!(matches!(
        accounts.put(&a, "saved/you", b"wrong account"),
        Err(Error::Locked)
    ));
    assert!(accounts.get(&b, "saved/you").unwrap().is_none());
    accounts.put(&b, "saved/you", b"b only").unwrap();
    let a2 = accounts.unlock(&a.did, "pass").unwrap();
    assert!(matches!(accounts.get(&a, "saved/you"), Err(Error::Locked)));
    assert_eq!(
        &**accounts.get(&a2, "saved/you").unwrap().as_ref().unwrap(),
        b"a only"
    );
    accounts.lock();
    assert!(accounts.current().is_none());
    assert!(matches!(accounts.get(&a2, "saved/you"), Err(Error::Locked)));
}

#[test]
fn duplicate_import_and_invalid_names_cannot_overwrite_records() {
    let dir = Dir::new();
    let accounts = Accounts::open(&dir.0).unwrap();
    assert!(matches!(
        accounts.create("a", "phone", ""),
        Err(Error::WeakPassphrase)
    ));
    assert!(accounts.accounts().unwrap().is_empty());
    let (a, phrase) = accounts.create("a", "phone", "pass").unwrap();
    let info = accounts.current().unwrap();
    assert!(
        accounts
            .import(&phrase, "replacement", "desktop", "pass")
            .is_err()
    );
    assert_eq!(accounts.current().unwrap(), info);
    for name in [
        "",
        "../identity/envelope",
        "identity//envelope",
        "/contacts",
        "contacts/",
    ] {
        assert!(accounts.put(&a, name, b"bad").is_err());
    }
    assert!(accounts.unlock("did:key:../../outside", "pass").is_err());
}

#[test]
fn substitution_and_tampering_fail_without_silently_resetting_data() {
    let dir = Dir::new();
    let accounts = Accounts::open(&dir.0).unwrap();
    let (a, _) = accounts.create("a", "phone", "pass").unwrap();
    accounts.put(&a, "contacts/main", b"contacts").unwrap();
    accounts.put(&a, "history/calls", b"history").unwrap();
    accounts.lock();
    {
        let store = storage::Store::open(dir.db(&a.did)).unwrap();
        let ct = store.get("data/contacts/main").unwrap().unwrap();
        store.put("data/history/calls", &ct).unwrap();
        let mut corrupt = ct;
        *corrupt.last_mut().unwrap() ^= 1;
        store.put("data/contacts/main", &corrupt).unwrap();
    }
    let session = accounts.unlock(&a.did, "pass").unwrap();
    assert!(accounts.get(&session, "contacts/main").is_err());
    assert!(accounts.get(&session, "history/calls").is_err());
}

#[cfg(unix)]
#[test]
fn private_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Dir::new();
    let accounts = Accounts::open(&dir.0).unwrap();
    let (a, _) = accounts.create("a", "phone", "pass").unwrap();
    assert_eq!(
        std::fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(dir.db(&a.did))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
