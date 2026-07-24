use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use stay::tmux::Tmux;

fn unique_namespace() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("stay-test-attach-{nanos}-{counter}")
}

fn unique_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("attach-{nanos}-{counter}")
}

struct SessionGuard {
    tmux: Tmux,
}

impl SessionGuard {
    fn new(namespace: String, name: &str) -> Self {
        let tmux = Tmux::for_test_namespace(namespace);
        let status = tmux
            .command(["new-session", "-d", "-s", name, "--", "sleep", "30"])
            .status()
            .expect("start attach test session");
        assert!(status.success(), "tmux failed to create test session");
        Self { tmux }
    }
}

struct TmuxShim {
    directory: PathBuf,
    real_tmux: PathBuf,
}

impl TmuxShim {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("stay-tmux-shim-{}", unique_name()));
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
            "#!/bin/sh\nif [ \"$1\" = \"-L\" ] && [ \"$2\" = \"stay\" ]; then\n    shift 2\n    set -- -L \"$STAY_TEST_NAMESPACE\" \"$@\"\nfi\nexec \"$STAY_TEST_REAL_TMUX\" \"$@\"\n",
        )
        .expect("write tmux shim");
        set_executable(&shim);

        Self {
            directory,
            real_tmux,
        }
    }

    fn path(&self) -> OsString {
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

#[cfg(unix)]
fn set_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .expect("read shim metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make shim executable");
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        let _ = self
            .tmux
            .command(["kill-server"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn wait_for_attached(tmux: &Tmux, name: &str, child: &mut Child) {
    for _ in 0..200 {
        if tmux
            .list_sessions()
            .expect("list isolated test sessions")
            .iter()
            .any(|session| session.name == name && session.attached)
        {
            return;
        }
        if let Some(status) = child.try_wait().expect("check stay status") {
            panic!("stay exited before attaching: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for stay to attach");
}

#[cfg(unix)]
#[test]
fn attaches_through_a_real_pty_and_detaches_with_tmux_prefix() {
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let executable_string = executable.to_string_lossy();
    let command_string = format!("{} {}", shell_quote(&executable_string), shell_quote(&name));

    let mut script = Command::new("script");
    if cfg!(target_os = "linux") {
        script.args(["-q", "-e", "-c", &command_string, "/dev/null"]);
    } else {
        script.args(["-q", "/dev/null"]);
        script.args([executable.to_str().expect("stay path is UTF-8"), &name]);
    }
    let mut child = script
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start stay under a PTY");

    wait_for_attached(&guard.tmux, &name, &mut child);
    thread::sleep(Duration::from_millis(2500));
    assert!(
        child
            .try_wait()
            .expect("check attached stay status")
            .is_none(),
        "stay attach ended before the user detached"
    );

    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x02d")
        .expect("send tmux prefix detach");
    let status = child.wait().expect("wait for detached stay");
    assert!(status.success(), "stay detach failed: {status}");
}

#[test]
fn rejects_trailing_words_for_an_existing_session_without_attaching() {
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .args([name.as_str(), "echo", "ignored"])
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .output()
        .expect("run stay with trailing command words");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("trailing command words"), "{stderr}");
    assert!(guard
        .tmux
        .list_sessions()
        .expect("list existing session")
        .iter()
        .any(|session| session.name == name && !session.attached));
}

#[test]
fn production_wrapper_keeps_the_runtime_namespace_fixed_to_stay() {
    let command = Tmux::production().attach_command("work");
    assert_eq!(command.get_args().collect::<Vec<_>>()[1], "stay");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
