use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_path(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{counter}", std::process::id()))
}

/// Owns a temporary test path and removes it during unwinding too.
pub(crate) struct TempPath {
    path: PathBuf,
}

impl TempPath {
    pub(crate) fn file(prefix: &str) -> Self {
        Self {
            path: unique_path(prefix),
        }
    }

    pub(crate) fn directory(prefix: &str) -> Self {
        let path = unique_path(prefix);
        fs::create_dir_all(&path).expect("create temporary test directory");
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.path()
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let is_directory =
            fs::symlink_metadata(&self.path).is_ok_and(|metadata| metadata.file_type().is_dir());
        if is_directory {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}
