//! The open-source libraries every Tinline build ships, with their licences, for Settings > About Tinline.
//! One list for the desktop and Android apps (each adds the UI libraries only it ships). The licences are
//! those the crates declare in `Cargo.toml` (checked with `cargo metadata`); libaom and Opus are the C
//! libraries behind the `video` and `audio` crates, whose licence texts ship with the package.

/// One library and its licence, in the words the About page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenSourceLicence {
    pub name: String,
    pub licence: String,
}

const LIBS: [(&str, &str); 10] = [
    ("iroh", "MIT or Apache-2.0"),
    ("Opus", "BSD-3-Clause"),
    ("rav1d", "BSD-2-Clause"),
    ("libaom", "BSD-2-Clause"),
    ("redb", "MIT or Apache-2.0"),
    ("BLAKE3", "CC0 or Apache-2.0"),
    ("Loro", "MIT"),
    ("ed25519-dalek", "BSD-3-Clause"),
    ("Tokio", "MIT"),
    ("UniFFI", "MPL-2.0"),
];

/// The libraries inside the core, shared by both apps.
#[uniffi::export]
pub fn open_source_licences() -> Vec<OpenSourceLicence> {
    LIBS.iter().map(|(n, l)| OpenSourceLicence { name: (*n).into(), licence: (*l).into() }).collect()
}

#[cfg(test)]
mod tests {
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
}
