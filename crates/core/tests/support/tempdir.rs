//! A per-test data directory that is removed when dropped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
static RUN: std::sync::LazyLock<u128> = std::sync::LazyLock::new(|| {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
});

/// A unique per-allocation directory, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    /// The path only: the directory is not created (`Node::new` creates what it needs).
    pub fn new(prefix: &str, name: &str) -> TempDir {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "p2pcore-{prefix}-{name}-{}-{}-{id}",
            std::process::id(),
            *RUN
        ));
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
