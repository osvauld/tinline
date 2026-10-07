use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tinline-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn migration_moves_identity_once() {
    let (old, new) = (tmp("old").join("data"), tmp("new").join("data"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("profile.json"), b"{\"keys\":1}").unwrap();
    std::fs::write(old.join("state.json"), b"{}").unwrap();
    assert!(migrate_data_dir(&old, &new).unwrap());
    assert_eq!(std::fs::read(new.join("profile.json")).unwrap(), b"{\"keys\":1}");
    assert!(!old.exists());
    // Second start: nothing to do.
    assert!(!migrate_data_dir(&old, &new).unwrap());
}

#[test]
fn migration_never_overwrites_an_existing_identity() {
    let (old, new) = (tmp("old2").join("data"), tmp("new2").join("data"));
    for d in [&old, &new] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("profile.json"), d.to_string_lossy().as_bytes()).unwrap();
    }
    assert!(!migrate_data_dir(&old, &new).unwrap());
    assert!(old.join("profile.json").exists());
    // A non-empty new dir without an identity is refused, not clobbered.
    std::fs::remove_file(new.join("profile.json")).unwrap();
    std::fs::write(new.join("other"), b"x").unwrap();
    assert!(migrate_data_dir(&old, &new).is_err());
    assert!(old.join("profile.json").exists());
}

#[test]
fn fresh_install_touches_nothing() {
    let (old, new) = (tmp("old3").join("data"), tmp("new3").join("data"));
    assert!(!migrate_data_dir(&old, &new).unwrap());
    assert!(!new.exists());
}
