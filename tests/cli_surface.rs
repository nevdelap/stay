#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use stay::tmux::Tmux;

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{counter}", std::process::id())
}

struct TmuxShim {
    directory: PathBuf,
    real_tmux: PathBuf,
}

impl TmuxShim {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("stay-cli-shim-{}", unique_suffix()));
        fs::create_dir(&directory).expect("create tmux shim directory");
        let real_tmux = Command::new("/bin/sh")
            .args(["-c", "command -v tmux"])
            .output()
            .expect("locate tmux")
            .stdout;
        let real_tmux = PathBuf::from(
            String::from_utf8(real_tmux)
                .expect("tmux path is UTF-8")
                .trim()
                .to_owned(),
        );
        assert!(real_tmux.is_file(), "tmux executable was not found");

        let shim = directory.join("tmux");
        fs::write(
            &shim,
            "#!/bin/sh
printf '%s\\n' \"$*\" >> \"$STAY_TEST_CALL_LOG\"
if [ \"$1\" = \"-L\" ] && [ \"$2\" = \"stay\" ]; then
    shift 2
    set -- -L \"$STAY_TEST_NAMESPACE\" \"$@\"
fi
exec \"$STAY_TEST_REAL_TMUX\" \"$@\"
",
        )
        .expect("write tmux shim");
        let mut permissions = fs::metadata(&shim)
            .expect("read shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shim, permissions).expect("make shim executable");

        Self {
            directory,
            real_tmux,
        }
    }

    fn path(&self) -> std::ffi::OsString {
        let mut paths = vec![self.directory.clone()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        std::env::join_paths(paths).expect("construct test PATH")
    }
}

impl Drop for TmuxShim {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.directory.join("tmux"));
        let _ = fs::remove_dir(&self.directory);
    }
}

struct ServerGuard {
    tmux: Tmux,
}

impl ServerGuard {
    fn new(namespace: &str) -> Self {
        Self {
            tmux: Tmux::for_test_namespace(namespace),
        }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self
            .tmux
            .command(["kill-server"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn run_stay(
    arguments: &[&str],
    namespace: &str,
    shim: &TmuxShim,
    call_log: &Path,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_stay"))
        .args(arguments)
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_CALL_LOG", call_log)
        .output()
        .expect("run stay")
}

#[test]
fn unimplemented_flags_fail_before_touching_tmux() {
    for (arguments, flag) in [
        (&["-r", "work"][..], "-r"),
        (&["-l", "work"][..], "-l"),
        (&["-p", "work"][..], "-p"),
        (&["-L", "f", "work"][..], "-L"),
        (&["-t", "-L", "f", "work"][..], "-t"),
        (&["-s", "-L", "f", "work"][..], "-s"),
    ] {
        let namespace = format!("stay-test-cli-{}", unique_suffix());
        let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
        let shim = TmuxShim::new();
        let server = ServerGuard::new(&namespace);
        let output = run_stay(arguments, &namespace, &shim, &call_log);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "stay unexpectedly succeeded");
        assert!(stderr.contains(flag), "stderr omitted {flag}: {stderr}");
        assert!(stderr.contains("not yet implemented"), "stderr: {stderr}");
        assert!(server.tmux.list_sessions().unwrap().is_empty());
        assert!(!call_log.exists(), "stay touched tmux: {stderr}");
        drop(server);
        let _ = fs::remove_file(call_log);
    }
}

#[test]
fn empty_session_name_fails_during_parse_without_touching_tmux() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let output = run_stay(&[""], &namespace, &shim, &call_log);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("invalid session name: must not be empty"));
    assert!(server.tmux.list_sessions().unwrap().is_empty());
    assert!(!call_log.exists(), "stay touched tmux: {stderr}");
    drop(server);
    let _ = fs::remove_file(call_log);
}
