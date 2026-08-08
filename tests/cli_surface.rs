#![cfg(unix)]

mod support;

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use stay::tmux::Tmux;
use support::{TempPath, TestEnvironment};

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
    directory: TempPath,
    real_tmux: PathBuf,
    environment: TestEnvironment,
}

impl TmuxShim {
    fn new() -> Self {
        let directory = TempPath::directory("stay-cli-shim");
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
if [ -n \"${STAY_TEST_FAIL_ATTACH:-}\" ] && [ \"$3\" = \"attach-session\" ]; then
    echo \"attach failed\" >&2
    exit 42
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
            environment: TestEnvironment::new(),
        }
    }

    fn path(&self) -> std::ffi::OsString {
        let mut paths = vec![self.directory.path().to_owned()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        std::env::join_paths(paths).expect("construct test PATH")
    }

    fn apply(&self, command: &mut Command) {
        self.environment.apply(command);
    }

    fn stay_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stay"));
        self.apply(&mut command);
        command
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
        let _ = self.tmux.run(["kill-server"]);
    }
}

fn run_stay(
    arguments: &[&str],
    namespace: &str,
    shim: &TmuxShim,
    call_log: &Path,
) -> std::process::Output {
    let mut command = shim.stay_command();
    command
        .args(arguments)
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_CALL_LOG", call_log)
        .output()
        .expect("run stay")
}

fn run_stay_with_attach_failure(
    arguments: &[&str],
    namespace: &str,
    shim: &TmuxShim,
    call_log: &Path,
) -> std::process::Output {
    let mut command = shim.stay_command();
    command
        .args(arguments)
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_CALL_LOG", call_log)
        .env("STAY_TEST_FAIL_ATTACH", "1")
        .output()
        .expect("run stay with attach failure")
}

fn wait_for_file(path: &Path) {
    for _ in 0..500 {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

fn create_retained_cli_session(tmux: &Tmux, name: &str, command: &str) {
    for (arguments, description) in [
        (
            vec!["new-session", "-d", "-s", name, "--", "sleep", "30"],
            "create retained CLI session",
        ),
        (
            vec!["set-window-option", "-t", name, "remain-on-exit", "on"],
            "retain CLI session",
        ),
        (
            vec!["respawn-pane", "-k", "-t", name, "sh", "-c", command],
            "start retained CLI command",
        ),
    ] {
        let output = tmux
            .run(arguments)
            .unwrap_or_else(|error| panic!("{description}: {error}"));
        assert!(
            output.status.success(),
            "{description}: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

fn run_tmux_success<I, S>(tmux: &Tmux, arguments: I, description: &str)
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = tmux
        .run(arguments)
        .unwrap_or_else(|error| panic!("{description}: {error}"));
    assert!(
        output.status.success(),
        "{description}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_for_terminated_session(tmux: &Tmux, name: &str, expected: u8) {
    // tmux can publish a retained pane's status to its target-pane query
    // before the wider inventory format is fully refreshed under parallel
    // test load.  Synchronize the fixture on the pane that actually exited;
    // the following `stay create -f` invocation still verifies the public
    // inventory path and its user-visible recreate notice.
    let deadline = Instant::now() + Duration::from_secs(10);
    let last_status = loop {
        let status = tmux
            .pane_exit_status(name)
            .expect("read terminated CLI pane status");
        if status == Some(expected) {
            return;
        }
        if Instant::now() >= deadline {
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    panic!(
        "timed out waiting for {name} to terminate with {expected}; last status: {last_status:?}\n{}",
        terminated_session_diagnostics(tmux, name)
    );
}

fn terminated_session_diagnostics(tmux: &Tmux, name: &str) -> String {
    fn query(tmux: &Tmux, label: &str, arguments: &[&str]) -> String {
        match tmux.run(arguments.iter().copied()) {
            Ok(output) => format!(
                "{label}: status={} stdout={:?} stderr={:?}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ),
            Err(error) => format!("{label}: command error: {error}"),
        }
    }

    let target = query(
        tmux,
        "target pane",
        [
            "list-panes",
            "-t",
            name,
            "-F",
            "#{pane_dead}:#{pane_dead_status}:#{pane_dead_time}:#{pane_dead_signal}:#{pane_pid}:#{pane_current_command}:#{pane_start_command}",
        ]
        .as_slice(),
    );
    let inventory = query(
        tmux,
        "all panes",
        [
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{pane_dead}:#{pane_dead_status}:#{pane_dead_time}:#{pane_dead_signal}:#{pane_pid}:#{pane_current_command}:#{pane_start_command}",
        ]
        .as_slice(),
    );
    let remain_on_exit = query(
        tmux,
        "remain-on-exit",
        ["show-window-options", "-t", name, "-v", "remain-on-exit"].as_slice(),
    );
    format!(
        "tmux failure diagnostics (socket root {}):\n{target}\n{inventory}\n{remain_on_exit}",
        stay::tmux::test_tmux_tmpdir().display(),
    )
}

fn wait_for_signalled_session(tmux: &Tmux, name: &str, expected: u8) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let last_status = loop {
        let output = tmux
            .run([
                "list-panes",
                "-t",
                name,
                "-F",
                "#{pane_dead}:#{pane_dead_signal}",
            ])
            .expect("list signalled CLI pane");
        assert!(
            output.status.success(),
            "list signalled CLI pane failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let current_status = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if current_status.lines().any(|row| {
            row == format!("1:{expected}")
                || row == format!("1:SIG{expected}")
                || (expected == 9 && row.eq_ignore_ascii_case("1:kill"))
        }) {
            return;
        }
        if Instant::now() >= deadline {
            break current_status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    panic!(
        "timed out waiting for {name} to terminate with signal {expected}; last pane status: {last_status:?}"
    );
}

fn wait_for_pane_pid(tmux: &Tmux, name: &str) -> Pid {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = tmux
            .run(["list-panes", "-t", name, "-F", "#{pane_pid}"])
            .expect("list signalled CLI pane pid");
        assert!(
            output.status.success(),
            "list signalled CLI pane pid failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if let Some(pid) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.trim().parse::<i32>().ok())
        {
            return Pid::from_raw(pid);
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {name} pane pid"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_live_and_terminated_sessions(
    tmux: &Tmux,
    live_name: &str,
    terminated_name: &str,
    expected: u8,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_sessions = Vec::new();
    loop {
        let sessions = tmux
            .list_sessions()
            .expect("list live and terminated CLI sessions");
        last_sessions.clone_from(&sessions);
        let has_live = sessions
            .iter()
            .any(|session| session.name == live_name && !session.terminated);
        let has_terminated = sessions.iter().any(|session| {
            session.name == terminated_name
                && session.terminated
                && session.exit_code == Some(expected)
        });
        if has_live && has_terminated {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "timed out waiting for live session {live_name} and terminated session {terminated_name}; last inventory: {last_sessions:?}"
    );
}

#[test]
fn empty_session_name_fails_during_parse_without_touching_tmux() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let output = run_stay(&["create", ""], &namespace, &shim, &call_log);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("invalid session name: must not be empty"));
    assert!(server.tmux.list_sessions().unwrap().is_empty());
    assert!(!call_log.exists(), "stay touched tmux: {stderr}");
    drop(server);
    let _ = fs::remove_file(call_log.path());
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
        let call_log = TempPath::file("stay-cli-log");
        let shim = TmuxShim::new();
        let server = ServerGuard::new(&namespace);
        let output = run_stay(arguments, &namespace, &shim, &call_log);
        assert!(!output.status.success(), "accepted old form {arguments:?}");
        assert_eq!(output.status.code(), Some(2));
        assert!(!call_log.exists(), "old form touched tmux: {arguments:?}");
        drop(server);
        let _ = fs::remove_file(call_log.path());
    }
}

#[test]
fn bare_non_tty_points_at_list() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let output = run_stay(&[], &namespace, &shim, &call_log);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("use `stay list`"), "stderr: {stderr}");
    drop(server);
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn create_attach_and_kill_are_explicit_and_strict() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
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
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn create_attachment_modifiers_require_attach_without_touching_tmux() {
    for flag in ["-r", "--read-only", "-L", "--low-priority"] {
        let namespace = format!("stay-test-cli-{}", unique_suffix());
        let call_log = TempPath::file("stay-cli-log");
        let shim = TmuxShim::new();
        let server = ServerGuard::new(&namespace);
        let output = run_stay(&["create", "work", flag], &namespace, &shim, &call_log);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "accepted detached modifier {flag}"
        );
        assert!(stderr.contains("require -a/--attach"), "stderr: {stderr}");
        assert!(server.tmux.list_sessions().unwrap().is_empty());
        assert!(!call_log.exists(), "rejected create touched tmux: {stderr}");
        drop(server);
        let _ = fs::remove_file(call_log.path());
    }
}

#[test]
fn create_and_attach_failure_leaves_the_created_session() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let output = run_stay_with_attach_failure(
        &["create", "work", "--attach"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(!output.status.success(), "attach failure returned success");
    assert!(
        server
            .tmux
            .list_sessions()
            .unwrap()
            .iter()
            .any(|session| session.name == "work"),
        "created session was rolled back: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(server);
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn list_json_reports_live_and_terminated_pane_state() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let ready = TempPath::file("stay-cli-json-ready");
    let release = TempPath::file("stay-cli-json-release");
    let completion = TempPath::file("stay-cli-json-completion");

    let terminated_command = format!(
        ": > {}; while test ! -e {}; do sleep .01; done; : > {}; sleep 1; exit 7",
        shell_quote(&ready.to_string_lossy()),
        shell_quote(&release.to_string_lossy()),
        shell_quote(&completion.to_string_lossy()),
    );
    run_tmux_success(
        &server.tmux,
        ["new-session", "-d", "-s", "live", "--", "sleep", "300"],
        "create live JSON session",
    );
    run_tmux_success(
        &server.tmux,
        ["new-session", "-d", "-s", "dead", "--", "sleep", "30"],
        "create terminated JSON session",
    );
    run_tmux_success(
        &server.tmux,
        ["set-window-option", "-t", "dead", "remain-on-exit", "on"],
        "retain terminated JSON session",
    );
    run_tmux_success(
        &server.tmux,
        [
            "respawn-pane",
            "-k",
            "-t",
            "dead",
            "sh",
            "-c",
            &terminated_command,
        ],
        "start terminated JSON command",
    );

    wait_for_file(&ready);
    fs::write(&release, b"").expect("release terminated JSON command");
    wait_for_file(&completion);
    wait_for_live_and_terminated_sessions(&server.tmux, "live", "dead", 7);
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
    assert!(stdout.contains("\"signal\":null"));
    assert!(stdout.contains(&format!(
        "\"current_directory\":\"{}\"",
        live.current_directory.unwrap()
    )));
    assert!(stdout.contains(&format!(
        "\"current_command\":\"{}\"",
        live.current_command.unwrap()
    )));

    drop(server);
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn list_json_accepts_a_colon_in_the_live_pane_directory() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let directory = TempPath::directory("stay-cli-dir:");

    let directory_string = directory.to_str().expect("colon pane directory is UTF-8");
    run_tmux_success(
        &server.tmux,
        [
            "new-session",
            "-d",
            "-s",
            "colon",
            "-c",
            directory_string,
            "--",
            "sleep",
            "10",
        ],
        "create colon-directory session",
    );

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
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn force_recreate_reports_the_terminated_session_cause() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);

    // A terminated session: force-recreating it must report its exit code.
    let died_ready = TempPath::file("stay-cli-died-ready");
    let died_release = TempPath::file("stay-cli-died-release");
    let died_complete = TempPath::file("stay-cli-died-complete");
    let died_inner_command = format!(
        ": > {}; while test ! -e {}; do sleep .01; done; : > {}; sleep 1; exit 5",
        shell_quote(&died_ready.to_string_lossy()),
        shell_quote(&died_release.to_string_lossy()),
        shell_quote(&died_complete.to_string_lossy()),
    );
    create_retained_cli_session(&server.tmux, "died", &died_inner_command);
    wait_for_file(&died_ready);
    fs::write(&died_release, "").expect("release terminated CLI command");
    wait_for_file(&died_complete);
    wait_for_terminated_session(&server.tmux, "died", 5);
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

    // A signal-killed session must preserve its signal cause instead of
    // fabricating the fallback exit code 0 in the recreate notice.
    create_retained_cli_session(&server.tmux, "signalled", "sleep 30");
    let signalled_pid = wait_for_pane_pid(&server.tmux, "signalled");
    kill(signalled_pid, Signal::SIGKILL).expect("kill signalled CLI pane");
    wait_for_signalled_session(&server.tmux, "signalled", 9);
    let output = run_stay(
        &["create", "signalled", "-f", "sleep", "30"],
        &namespace,
        &shim,
        &call_log,
    );
    assert!(output.status.success(), "force-recreate failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("signal=9"), "{stderr}");
    assert!(!stderr.contains("exit code 0"), "{stderr}");

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
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn pass_through_against_a_nonexistent_session_errors_without_creating_one() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
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
    let _ = fs::remove_file(call_log.path());
}

#[test]
fn pass_through_delivers_a_streaming_producer_incrementally() {
    let namespace = format!("stay-test-cli-{}", unique_suffix());
    let call_log = TempPath::file("stay-cli-log");
    let shim = TmuxShim::new();
    let server = ServerGuard::new(&namespace);
    let root = TempPath::directory("stay-cli-passthrough");
    let marker = root.join("received.txt");
    let script = format!(
        "for i in 1 2; do IFS= read -r line; printf '%s\\n' \"$line\" >> {}; done; sleep 30",
        shell_quote(&marker.to_string_lossy())
    );
    run_tmux_success(
        &server.tmux,
        [
            "new-session",
            "-d",
            "-s",
            "streaming",
            "--",
            "sh",
            "-c",
            &script,
        ],
        "create streaming target session",
    );

    let mut child = shim
        .stay_command()
        .args(["attach", "streaming", "-p"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_CALL_LOG", call_log.path())
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
    for _ in 0..500 {
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
    let _ = fs::remove_file(call_log.path());
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
