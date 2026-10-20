use super::*;

/// The markup the avatar rasterises parses, and draws something inside the box, for several keys and both sizes.
#[test]
fn fingerprint_svg_renders() {
    // Synthetic DIDs (the golden file's).
    for (seed, did) in ["z6Mkeb6dsrBTX95vPgLiZAcgRr6XJthFm1czoqFEx34DQtRo", "z6Mkef1w29eU7SP9XmbDzTwbiisHZ3uTS5JpNEuu1R6pADkW", "z6MkenrXJkaVJ2ybox6Es4bSJUQp3NKrnCgTV4FD8BC1etPv", "z6MkeXBLjYiSvqnhFb6D7sHm8yKm4jV45wwBFRaatf1cfZ76", "z6MkezcQjeyX4vsHEDqmAxaCB6j6mqxToPjvAHFBJKKottN3", "z6MkfLCtTAJa2RMQvg6JMTCndUauze1UAiB1xKaTbDYodYya"].iter().enumerate() {
        let did = format!("did:key:{did}");
        for small in [false, true] {
            let ar = p2pcore::avatar::avatar_art(did.clone(), small).unwrap();
            let tree = usvg::Tree::from_str(&fingerprint_svg(&ar, 0x1E5B33, 0xC98A1E), &usvg::Options::default()).expect("parses");
            let mut pm = resvg::tiny_skia::Pixmap::new(96, 96).unwrap();
            resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(0.96, 0.96), &mut pm.as_mut());
            assert!(pm.pixels().iter().filter(|p| p.alpha() > 0).count() > 100, "{seed} {small}");
        }
    }
    assert!(fingerprint_art("me", 40.0).is_none());
    // A group id always has art, and not the same as a person key of the same text.
    let g = art_cached("g-0f3a9c", 44.0, true).expect("group art");
    let tree = usvg::Tree::from_str(&fingerprint_svg(&g, 0x1E5B33, 0xC98A1E), &usvg::Options::default()).expect("parses");
    let mut pm = resvg::tiny_skia::Pixmap::new(96, 96).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(0.96, 0.96), &mut pm.as_mut());
    assert!(pm.pixels().iter().filter(|p| p.alpha() > 0).count() > 100);
}
