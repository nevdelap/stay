#![allow(dead_code, unsafe_code)]

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_path(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Unix socket paths are limited; macOS's normal temp path is too long
    // once tmux appends its UID directory and namespace.
    #[cfg(unix)]
    let root = PathBuf::from("/tmp");
    #[cfg(not(unix))]
    let root = std::env::temp_dir();
    root.join(format!("{prefix}-{}-{nanos}-{counter}", std::process::id()))
}

/// Owns a temporary file or directory and removes it during unwinding too.
pub struct TempPath {
    path: PathBuf,
}

impl TempPath {
    pub fn file(prefix: impl AsRef<str>) -> Self {
        Self {
            path: unique_path(prefix.as_ref()),
        }
    }

    pub fn directory(prefix: impl AsRef<str>) -> Self {
        let path = unique_path(prefix.as_ref());
        fs::create_dir_all(&path).expect("create temporary directory");
        Self { path }
    }

    pub fn short_directory() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        #[cfg(unix)]
        let root = PathBuf::from("/tmp");
        #[cfg(not(unix))]
        let root = std::env::temp_dir();
        let path = root.join(format!(
            "st{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create short temporary directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
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
        if self.path.is_dir() {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Gives a spawned process a clean configuration environment.
pub struct TestEnvironment {
    root: TempPath,
    home: PathBuf,
    config: PathBuf,
}

impl TestEnvironment {
    pub fn new() -> Self {
        let root = TempPath::directory("stay-test-environment");
        let home = root.path().join("home");
        let config = root.path().join("config");
        fs::create_dir(&home).expect("create test home");
        fs::create_dir(&config).expect("create test config directory");
        Self { root, home, config }
    }

    pub fn apply(&self, command: &mut Command) {
        command
            .env_remove("TMUX")
            .env_remove("STAY_CMD")
            .env_remove("STAY_DETACH_KEY")
            .env_remove("STAY_COPY_MODE_KEY")
            .env_remove("STAY_HISTORY_LINES")
            .env_remove("STAY_LOG_CAPTURE_INTERVAL_SECONDS")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("TMUX_TMPDIR", stay::tmux::test_tmux_tmpdir());
    }

    pub fn stay_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stay"));
        self.apply(&mut command);
        command
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn config(&self) -> &Path {
        &self.config
    }
}
