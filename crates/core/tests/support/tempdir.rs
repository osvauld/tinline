//! A per-test data directory that is removed when dropped.

use std::path::{Path, PathBuf};

/// `<tmp>/p2pcore-<prefix>-<name>-<pid>`, cleared first and removed again on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    /// The path only: the directory is not created (`Node::new` creates what it needs).
    pub fn new(prefix: &str, name: &str) -> TempDir {
        let p =
            std::env::temp_dir().join(format!("p2pcore-{prefix}-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        TempDir(p)
    }

    /// As `new`, and the directory exists (empty).
    pub fn created(prefix: &str, name: &str) -> TempDir {
        let d = TempDir::new(prefix, name);
        std::fs::create_dir_all(&d.0).unwrap();
        d
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
