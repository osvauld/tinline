use super::*;

#[test]
fn list_is_named_unique_and_licensed() {
    let l = open_source_licences();
    assert!(l.len() >= 6);
    for (i, a) in l.iter().enumerate() {
        assert!(!a.name.is_empty() && !a.licence.is_empty());
        assert!(l[i + 1..].iter().all(|b| b.name != a.name), "{} twice", a.name);
    }
}
