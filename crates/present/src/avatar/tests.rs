use super::*;

/// Published FNV-1a 32-bit test vectors.
#[test]
fn hash_golden() {
    for (input, want) in [("", 0x811c9dc5), ("a", 0xe40c292c), ("foobar", 0xbf9cf968)] {
        assert_eq!(stable_hash(input.into()), want, "{input:?}");
    }
}

#[test]
fn slot_golden() {
    for (input, slots, want) in [
        (
            "did:key:z6MkAnn",
            4,
            stable_hash("did:key:z6MkAnn".into()) % 4,
        ),
        ("a", 4, 0xe40c292c % 4),
        ("foobar", 4, 0xbf9cf968 % 4),
        ("anything", 0, 0),
        ("anything", 1, 0),
    ] {
        assert_eq!(avatar_slot(input.into(), slots), want, "{input:?}");
    }
    // Pinned values: if these change, every contact's colour changes.
    assert_eq!(avatar_slot("a".into(), 4), 0);
    assert_eq!(avatar_slot("foobar".into(), 4), 0);
    assert_eq!(avatar_slot("did:key:z6MkAnn".into(), 4), 2);
}

#[test]
fn every_slot_is_used_and_in_range() {
    let mut seen = [false; 4];
    for i in 0..200 {
        let s = avatar_slot(format!("did:key:z6Mk{i}"), 4);
        assert!(s < 4);
        seen[s as usize] = true;
    }
    assert!(seen.iter().all(|x| *x));
}
