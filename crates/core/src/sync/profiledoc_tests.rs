//! Real-Loro document tests, not network or blob-transfer acceptance tests.
use super::profiledoc::ProfileDoc;
use serde_json::json;

fn exchange(a: &ProfileDoc, b: &ProfileDoc) {
    let av = a.vv();
    let bv = b.vv();
    let ab = a.export_since(&bv);
    let ba = b.export_since(&av);
    if let Some((bytes, _)) = ab {
        b.import(&bytes).unwrap();
    }
    if let Some((bytes, _)) = ba {
        a.import(&bytes).unwrap();
    }
}

#[test]
fn empty_profile_and_explicit_patch_delete() {
    let p = ProfileDoc::new(&[1; 32]);
    assert!(p.fields().unwrap().is_empty());
    assert!(p.set("name", json!("Alice")).unwrap());
    assert!(p.set("status", json!("Reading")).unwrap());
    assert!(!p.set("name", json!("Alice")).unwrap());
    assert!(p.remove("status").unwrap());
    assert!(!p.remove("status").unwrap());
    assert_eq!(p.fields().unwrap().get("name"), Some(&json!("Alice")));
    assert!(!p.fields().unwrap().contains_key("status"));
}

#[test]
fn unknown_structured_fields_survive_snapshot_and_sync() {
    let a = ProfileDoc::new(&[1; 32]);
    a.set("example:interests", json!({"topics": ["music", "hiking"]}))
        .unwrap();
    let b = ProfileDoc::load(&[2; 32], Some(&a.snapshot())).unwrap();
    b.set("name", json!("Alice")).unwrap();
    exchange(&a, &b);
    assert_eq!(a.fields().unwrap(), b.fields().unwrap());
    assert_eq!(
        a.fields().unwrap()["example:interests"],
        json!({"topics": ["music", "hiking"]})
    );
}

#[test]
fn independent_concurrent_edits_merge() {
    let a = ProfileDoc::new(&[1; 32]);
    a.set("name", json!("Alice")).unwrap();
    let b = ProfileDoc::load(&[2; 32], Some(&a.snapshot())).unwrap();
    a.set("status", json!("Reading")).unwrap();
    b.set("availability", json!("paused")).unwrap();
    exchange(&a, &b);
    assert_eq!(a.fields().unwrap(), b.fields().unwrap());
    assert_eq!(a.fields().unwrap()["status"], json!("Reading"));
    assert_eq!(a.fields().unwrap()["availability"], json!("paused"));
}

#[test]
fn concurrent_same_field_edits_converge_regardless_of_delivery_order() {
    let a = ProfileDoc::new(&[1; 32]);
    a.set("status", json!("Initial")).unwrap();
    let snapshot = a.snapshot();
    let b = ProfileDoc::load(&[2; 32], Some(&snapshot)).unwrap();
    let c = ProfileDoc::load(&[3; 32], Some(&snapshot)).unwrap();
    let d = ProfileDoc::load(&[4; 32], Some(&snapshot)).unwrap();
    let base = c.vv();
    a.set("status", json!("Reading")).unwrap();
    b.set("status", json!("Walking")).unwrap();
    let ab = a.export_since(&base).unwrap().0;
    let bb = b.export_since(&base).unwrap().0;
    c.import(&ab).unwrap();
    c.import(&bb).unwrap();
    d.import(&bb).unwrap();
    d.import(&ab).unwrap();
    assert_eq!(c.fields().unwrap(), d.fields().unwrap());
}

#[test]
fn offline_device_catches_up_after_restart() {
    let a = ProfileDoc::new(&[1; 32]);
    a.set("name", json!("Alice")).unwrap();
    let b = ProfileDoc::load(&[2; 32], Some(&a.snapshot())).unwrap();
    a.set("status", json!("First")).unwrap();
    a.set("status", json!("Latest")).unwrap();
    let restarted = ProfileDoc::load(&[1; 32], Some(&a.snapshot())).unwrap();
    let (bytes, _) = restarted.export_since(&b.vv()).unwrap();
    assert!(b.import(&bytes).unwrap());
    assert!(!b.import(&bytes).unwrap());
    assert_eq!(b.fields().unwrap(), restarted.fields().unwrap());
    assert!(restarted.export_since(&b.vv()).is_none());
}

#[test]
fn picture_reference_replacement_and_removal_catch_up() {
    let a = ProfileDoc::new(&[1; 32]);
    // Opaque fixture references: this test does not define the wire picture descriptor.
    a.set("picture", json!({"hash": "fixture-one"})).unwrap();
    let b = ProfileDoc::load(&[2; 32], Some(&a.snapshot())).unwrap();
    a.set("picture", json!({"hash": "fixture-two"})).unwrap();
    exchange(&a, &b);
    assert_eq!(b.fields().unwrap()["picture"]["hash"], "fixture-two");
    a.remove("picture").unwrap();
    exchange(&a, &b);
    assert!(!b.fields().unwrap().contains_key("picture"));
    assert_eq!(a.fields().unwrap(), b.fields().unwrap());
}
