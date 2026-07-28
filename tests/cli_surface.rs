#![cfg(unix)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
fn empty_session_name_fails_during_parse_without_touching_tmux() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let output = run_stay(&["create", ""], &namespace, &shim, &call_log);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("invalid session name: must not be empty"));
    assert!(server.tmux.list_sessions().unwrap().is_empty());
    assert!(!call_log.exists(), "stay touched tmux: {stderr}");
    drop(server);
    let _ = fs::remove_file(call_log);
}

#[test]
fn old_flat_forms_are_rejected_without_touching_tmux() {
    for arguments in [
        &["work"][..],
        &["-k", "work"][..],
        &["-f", "work"][..],
        &["work", "echo", "hi"][..],
    ] {
        let namespace = format!("stay-test-cli-{}", unique_suffix());
        let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
        let shim = TmuxShim::new();
        let server = ServerGuard::new(&namespace);
        let output = run_stay(arguments, &namespace, &shim, &call_log);
        assert!(!output.status.success(), "accepted old form {arguments:?}");
        assert!(!call_log.exists(), "old form touched tmux: {arguments:?}");
        drop(server);
        let _ = fs::remove_file(call_log);
    }
}

#[test]
fn bare_non_tty_points_at_list() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let output = run_stay(&[], &namespace, &shim, &call_log);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("use `stay list`"), "stderr: {stderr}");
    drop(server);
    let _ = fs::remove_file(call_log);
}

#[test]
fn create_attach_and_kill_are_explicit_and_strict() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);

    let output = run_stay(
        &["create", "work", "sleep", "10"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(
        output.status.success(),
        "create failed: {:?}",
        output.stderr
    );

    let output = run_stay(&["create", "work"], &namespace, &shim, &call_log);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));

    let output = run_stay(&["attach", "missing"], &namespace, &shim, &call_log);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not exist"));

    let output = run_stay(&["kill", "work"], &namespace, &shim, &call_log);
    assert!(output.status.success(), "kill failed: {:?}", output.stderr);
    drop(server);
    let _ = fs::remove_file(call_log);
}

#[test]
fn list_json_reports_live_and_terminated_pane_state() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);

    let status = server
        .tmux
        .command(["new-session", "-d", "-s", "live", "--", "sleep", "10"])
        .status()
        .expect("create live JSON session");
    assert!(status.success());
    let status = server
        .tmux
        .command([
            "new-session",
            "-d",
            "-s",
            "dead",
            "--",
            "sh",
            "-c",
            "sleep 1; exit 7",
        ])
        .status()
        .expect("create terminated JSON session");
    assert!(status.success());
    let status = server
        .tmux
        .command(["set-option", "-t", "dead", "remain-on-exit", "on"])
        .status()
        .expect("retain terminated JSON session");
    assert!(status.success());

    for _ in 0..250 {
        if server.tmux.pane_exit_status("dead").unwrap() == Some(7) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let live = server
        .tmux
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|session| session.name == "live")
        .expect("live JSON session row");
    assert!(live.current_directory.is_some());
    assert!(live.current_command.is_some());

    let output = run_stay(&["list", "--json"], &namespace, &shim, &call_log);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "list --json failed: {:?}",
        output.stderr
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "JSON output contains ANSI: {stdout}"
    );
    assert!(stdout.starts_with("{\"sessions\":["));
    assert!(stdout.contains("\"status\":\"terminated\""));
    assert!(stdout.contains("\"current_directory\":null"));
    assert!(stdout.contains("\"exit_code\":7"));
    assert!(stdout.contains(&format!(
        "\"current_directory\":\"{}\"",
        live.current_directory.unwrap()
    )));
    assert!(stdout.contains(&format!(
        "\"current_command\":\"{}\"",
        live.current_command.unwrap()
    )));

    drop(server);
    let _ = fs::remove_file(call_log);
}

#[test]
fn list_json_accepts_a_colon_in_the_live_pane_directory() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let directory = std::env::temp_dir().join(format!("stay-cli-dir:{}", unique_suffix()));
    fs::create_dir(&directory).expect("create colon-containing pane directory");

    let directory_string = directory.to_str().expect("colon pane directory is UTF-8");
    let status = server
        .tmux
        .command([
            "new-session",
            "-d",
            "-s",
            "colon",
            "-c",
            directory_string,
            "--",
            "sleep",
            "10",
        ])
        .status()
        .expect("create colon-directory session");
    assert!(status.success());

    let output = run_stay(&["list", "--json"], &namespace, &shim, &call_log);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "list --json failed: {:?}",
        output.stderr
    );
    let reported_directory = server
        .tmux
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|session| session.name == "colon")
        .and_then(|session| session.current_directory)
        .expect("colon session directory");
    assert!(reported_directory.contains(':'), "{reported_directory}");
    assert!(stdout.contains(&format!("\"current_directory\":\"{reported_directory}\"")));

    drop(server);
    let _ = fs::remove_dir(&directory);
    let _ = fs::remove_file(call_log);
}

#[test]
fn force_recreate_reports_a_terminated_sessions_exit_code_only() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);

    // A terminated session: force-recreating it must report its exit code.
    let output = run_stay(
        &["create", "died", "--", "sh", "-c", "sleep 1; exit 5"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(output.status.success(), "create failed: {output:?}");
    for _ in 0..250 {
        if server.tmux.pane_exit_status("died").unwrap() == Some(5) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let output = run_stay(
        &["create", "died", "-f", "sleep", "30"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(output.status.success(), "force-recreate failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("\"died\""), "{stderr}");
    assert!(stderr.contains("exit code 5"), "{stderr}");

    // A live session: force-recreating it reports nothing extra.
    let output = run_stay(
        &["create", "alive", "sleep", "30"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(output.status.success(), "create failed: {output:?}");
    let output = run_stay(
        &["create", "alive", "-f", "sleep", "30"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(output.status.success(), "force-recreate failed: {output:?}");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);

    // A nonexistent session: force-"recreating" it (a plain create) also
    // reports nothing extra.
    let output = run_stay(
        &["create", "never-existed", "-f", "sleep", "30"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(output.status.success(), "force-recreate failed: {output:?}");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);

    drop(server);
    let _ = fs::remove_file(call_log);
}

#[test]
fn pass_through_against_a_nonexistent_session_errors_without_creating_one() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);

    let output = run_stay(
        &["attach", "never-existed", "-p"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not exist"));
    assert!(server.tmux.list_sessions().unwrap().is_empty());

    drop(server);
    let _ = fs::remove_file(call_log);
}

#[test]
fn pass_through_delivers_a_streaming_producer_incrementally() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = std::env::temp_dir().join(format!("stay-cli-log-{}", unique_suffix()));
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let root = std::env::temp_dir().join(format!("stay-cli-passthrough-{}", unique_suffix()));
    fs::create_dir(&root).expect("create streaming marker directory");
    let marker = root.join("received.txt");
    let script = format!(
        "for i in 1 2; do IFS= read -r line; printf '%s\\n' \"$line\" >> {}; done; sleep 30",
        shell_quote(&marker.to_string_lossy())
    );
    let status = server
        .tmux
        .command([
            "new-session",
            "-d",
            "-s",
            "streaming",
            "--",
            "sh",
            "-c",
            &script,
        ])
        .status()
        .expect("create streaming target session");
    assert!(status.success());

    let mut child = Command::new(env!("CARGO_BIN_EXE_stay"))
        .args(["attach", "streaming", "-p"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_CALL_LOG", &call_log)
        .spawn()
        .expect("start pass-through stay");
    let mut stdin = child.stdin.take().expect("pass-through stdin");

    stdin
        .write_all(b"first\n")
        .expect("write first pass-through line");
    stdin.flush().expect("flush first pass-through line");

    // This is the assertion that actually proves the "not
    // buffered-to-EOF" requirement: the first line must land before the
    // producer ever writes the second line, let alone closes stdin.
    let mut first_seen = false;
    for _ in 0..250 {
        if fs::read_to_string(&marker).is_ok_and(|content| content.contains("first")) {
            first_seen = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(first_seen, "first line was not delivered incrementally");

    stdin
        .write_all(b"second\n")
        .expect("write second pass-through line");
    drop(stdin);

    let status = child.wait().expect("wait for pass-through stay");
    assert!(status.success(), "pass-through failed: {status:?}");
    let content = fs::read_to_string(&marker).expect("read streaming marker");
    assert_eq!(content, "first\nsecond\n");

    drop(server);
    let _ = fs::remove_file(&marker);
    let _ = fs::remove_dir(&root);
    let _ = fs::remove_file(call_log);
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
