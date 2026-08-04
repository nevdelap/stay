use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use stay::{config::Config, session, tmux::Tmux};

mod support;
use support::{TempPath, TestEnvironment};

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

fn pty_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn pty_script(executable: &std::path::Path, name: &str, shim: &TmuxShim) -> Command {
    let mut script = Command::new("script");
    if cfg!(target_os = "linux") {
        let command_string = format!(
            "{} attach {}",
            shell_quote(&executable.to_string_lossy()),
            shell_quote(name)
        );
        script.args(["-q", "-e", "-c", &command_string, "/dev/null"]);
    } else {
        script.args(["-q", "/dev/null"]);
        script.args([
            executable.to_str().expect("stay path is UTF-8"),
            "attach",
            name,
        ]);
    }
    shim.apply(&mut script);
    script
}

fn pty_shell_script(command: &str, shim: &TmuxShim) -> Command {
    let mut script = Command::new("script");
    if cfg!(target_os = "linux") {
        script.args(["-q", "-e", "-c", command, "/dev/null"]);
    } else {
        script.args(["-q", "/dev/null", "/bin/sh", "-c", command]);
    }
    shim.apply(&mut script);
    script
}

fn wait_for_file_contents(path: &std::path::Path, expected: &str) {
    wait_for_file_contents_with_attempts(path, expected, 200);
}

fn wait_for_file_contents_with_attempts(path: &std::path::Path, expected: &str, attempts: usize) {
    for _ in 0..attempts {
        if let Ok(contents) = fs::read_to_string(path)
            && contents == expected
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "timed out waiting for {} to contain {expected:?}",
        path.display()
    );
}

fn wait_for_nonempty_file(path: &std::path::Path) -> String {
    for _ in 0..200 {
        if let Ok(contents) = fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {} to contain text", path.display());
}

fn wait_for_output_contains(output: &Arc<Mutex<Vec<u8>>>, expected: &str) {
    for _ in 0..200 {
        let observed = output.lock().expect("lock picker output");
        if String::from_utf8_lossy(&observed).contains(expected) {
            return;
        }
        drop(observed);
        thread::sleep(Duration::from_millis(20));
    }
    let observed = output.lock().expect("lock picker output");
    panic!(
        "timed out waiting for picker output to contain {expected:?}; output: {:?}",
        String::from_utf8_lossy(&observed)
    );
}

fn output_since(output: &Arc<Mutex<Vec<u8>>>, start: usize) -> String {
    let observed = output.lock().expect("lock picker output");
    strip_csi_sequences(&observed[start..])
}

fn wait_for_output_occurrences(output: &Arc<Mutex<Vec<u8>>>, expected: &str, count: usize) {
    for _ in 0..200 {
        let observed = output.lock().expect("lock picker output");
        let occurrences = String::from_utf8_lossy(&observed).matches(expected).count();
        if occurrences >= count {
            return;
        }
        drop(observed);
        thread::sleep(Duration::from_millis(20));
    }
    let observed = output.lock().expect("lock picker output");
    panic!(
        "timed out waiting for {count} occurrences of {expected:?}; output: {:?}",
        String::from_utf8_lossy(&observed)
    );
}

fn wait_for_output_occurrences_after(
    output: &Arc<Mutex<Vec<u8>>>,
    expected: &str,
    previous_count: usize,
) {
    for _ in 0..200 {
        let observed = output.lock().expect("lock picker output");
        let occurrences = String::from_utf8_lossy(&observed).matches(expected).count();
        if occurrences > previous_count {
            return;
        }
        drop(observed);
        thread::sleep(Duration::from_millis(20));
    }
    let observed = output.lock().expect("lock picker output");
    panic!(
        "timed out waiting for another occurrence of {expected:?}; output: {:?}",
        String::from_utf8_lossy(&observed)
    );
}

fn start_output_reader(
    child: &mut Child,
    label: &str,
) -> (Arc<Mutex<Vec<u8>>>, thread::JoinHandle<()>) {
    let stdout = child
        .stdout
        .take()
        .unwrap_or_else(|| panic!("{label} stdout"));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_for_thread = Arc::clone(&observed);
    let thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => observed_for_thread
                    .lock()
                    .expect("lock picker output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read picker output: {error}"),
            }
        }
    });
    (observed, thread)
}

fn write_picker_input(child: &mut Child, input: &[u8]) {
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(input)
        .expect("write picker input");
}

fn strip_csi_sequences(value: &[u8]) -> String {
    let mut text = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] == 0x1b && value.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < value.len() {
                let byte = value[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else {
            text.push(value[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

struct SessionGuard {
    tmux: Tmux,
}

impl SessionGuard {
    fn empty(namespace: String) -> Self {
        Self {
            tmux: Tmux::for_test_namespace(namespace),
        }
    }

    fn new(namespace: String, name: &str) -> Self {
        Self::new_with_command(namespace, name, &["sleep", "30"])
    }

    fn new_with_command(namespace: String, name: &str, command_words: &[&str]) -> Self {
        let tmux = Tmux::for_test_namespace(namespace);
        let mut arguments = vec!["new-session", "-d", "-s", name, "--"];
        arguments.extend(command_words.iter().copied());
        let status = tmux
            .command(arguments)
            .status()
            .expect("start attach test session");
        assert!(status.success(), "tmux failed to create test session");
        let status = tmux
            .command(["set-option", "-t", name, "remain-on-exit", "on"])
            .status()
            .expect("enable remain-on-exit for attach test session");
        assert!(
            status.success(),
            "tmux failed to retain attach test session"
        );
        Self { tmux }
    }
}

struct TmuxShim {
    directory: TempPath,
    real_tmux: PathBuf,
    environment: TestEnvironment,
}

impl TmuxShim {
    fn new() -> Self {
        let directory = TempPath::directory("stay-tmux-shim");
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
            "#!/bin/sh\nif [ \"$1\" = \"-L\" ] && [ \"$2\" = \"stay\" ]; then\n    shift 2\n    set -- -L \"$STAY_TEST_NAMESPACE\" \"$@\"\nfi\nif [ -n \"${STAY_TEST_FAIL_LIST_FILE:-}\" ] && [ -f \"$STAY_TEST_FAIL_LIST_FILE\" ] && [ \"$3\" = \"list-panes\" ]; then\n    echo \"picker poll failed\" >&2\n    exit 1\nfi\nif [ -n \"${STAY_TEST_FAIL_ATTACH_FILE:-}\" ] && [ -f \"$STAY_TEST_FAIL_ATTACH_FILE\" ] && [ \"$3\" = \"attach-session\" ]; then\n    echo \"picker attach failed\" >&2\n    exit 1\nfi\nexec \"$STAY_TEST_REAL_TMUX\" \"$@\"\n",
        )
        .expect("write tmux shim");
        set_executable(&shim);

        Self {
            directory,
            real_tmux,
            environment: TestEnvironment::new(),
        }
    }

    fn path(&self) -> OsString {
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

fn client_count(tmux: &Tmux, session_name: &str) -> usize {
    let output = tmux
        .run(["list-clients", "-F", "#{client_session}"])
        .expect("list tmux clients");
    assert!(output.status.success(), "tmux failed to list clients");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|session| *session == session_name)
        .count()
}

fn pane_pid(tmux: &Tmux, session_name: &str) -> Pid {
    let output = tmux
        .run(["display-message", "-p", "-t", session_name, "#{pane_pid}"])
        .expect("query pane pid");
    assert!(output.status.success(), "tmux failed to query pane pid");
    let pid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i32>()
        .expect("pane pid is an integer");
    Pid::from_raw(pid)
}

fn wait_for_status_label(tmux: &Tmux, session_name: &str, child: &mut Child, label: &str) {
    for _ in 0..200 {
        let output = tmux
            .run(["list-clients", "-F", "#{client_name}|#{client_session}"])
            .expect("list tmux clients");
        assert!(output.status.success(), "tmux failed to list clients");
        for client in String::from_utf8_lossy(&output.stdout).lines() {
            let Some((client_name, client_session)) = client.split_once('|') else {
                continue;
            };
            if client_session != session_name {
                continue;
            }
            let status = tmux
                .run([
                    "display-message",
                    "-p",
                    "-t",
                    client_name,
                    "#{E:status-left}",
                ])
                .expect("read tmux client status");
            assert!(
                status.status.success(),
                "tmux failed to render client status"
            );
            if String::from_utf8_lossy(&status.stdout).contains(label) {
                return;
            }
        }
        if let Some(status) = child.try_wait().expect("check picker status") {
            panic!("picker exited before showing {label}: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {label} in tmux status");
}

fn wait_for_status_without_modifier_labels(tmux: &Tmux, session_name: &str, child: &mut Child) {
    for _ in 0..200 {
        let output = tmux
            .run(["list-clients", "-F", "#{client_name}|#{client_session}"])
            .expect("list tmux clients");
        assert!(output.status.success(), "tmux failed to list clients");
        for client in String::from_utf8_lossy(&output.stdout).lines() {
            let Some((client_name, client_session)) = client.split_once('|') else {
                continue;
            };
            if client_session != session_name {
                continue;
            }
            let status = tmux
                .run([
                    "display-message",
                    "-p",
                    "-t",
                    client_name,
                    "#{E:status-left}",
                ])
                .expect("read tmux client status");
            assert!(
                status.status.success(),
                "tmux failed to render client status"
            );
            let status = String::from_utf8_lossy(&status.stdout);
            assert!(!status.contains("(view only)"));
            assert!(!status.contains("(low priority)"));
            assert!(!status.contains("(view only / low priority)"));
            return;
        }
        if let Some(status) = child.try_wait().expect("check plain create attach status") {
            panic!("stay exited before showing plain status: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for plain status in tmux status");
}

fn wait_for_pane_exit_status(tmux: &Tmux, name: &str, expected: u8) {
    // CI can occasionally take several seconds to start the tmux server and
    // schedule the pane command, even though the command itself sleeps for
    // only one second. Keep this bounded while leaving enough time to observe
    // remain-on-exit and its retained status.
    for _ in 0..500 {
        if tmux.pane_exit_status(name).expect("read pane exit status") == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for pane {name} to exit with {expected}");
}

fn wait_for_terminated_session(tmux: &Tmux, name: &str, expected: u8) {
    for _ in 0..500 {
        if tmux
            .list_sessions()
            .expect("list terminated picker session")
            .iter()
            .any(|session| {
                session.name == name && session.terminated && session.exit_code == Some(expected)
            })
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for session {name} to terminate");
}

fn wait_for_child_status(child: &mut Child) -> std::process::ExitStatus {
    for _ in 0..500 {
        if let Some(status) = child.try_wait().expect("check child status") {
            return status;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    panic!("timed out waiting for child to exit");
}

#[cfg(unix)]
#[test]
fn attaches_through_a_real_pty_and_detaches_with_stay_key() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
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
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for detached stay");
    assert!(status.success(), "stay detach failed: {status}");
}

#[cfg(unix)]
#[test]
fn relay_forwards_a_large_input_while_pane_is_busy() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create busy relay directory");
    let received = root.join("received");
    let received_string = shell_quote(&received.to_string_lossy());
    let command = format!("while :; do printf busy-output; done & exec cat > {received_string}");
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start busy relay test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    let payload = "input-byte\n".repeat(1024 * 1024 / 11 + 1);
    let payload_for_writer = payload.clone();
    let mut stdin = child.stdin.take().expect("busy relay stdin");
    let writer = thread::spawn(move || {
        stdin
            .write_all(payload_for_writer.as_bytes())
            .map(|()| stdin)
    });

    wait_for_file_contents_with_attempts(&received, &payload, 1000);
    let mut stdin = writer
        .join()
        .expect("join busy relay input writer")
        .expect("write busy relay input");
    stdin
        .write_all(b"\x1c")
        .expect("send busy relay detach key");
    let status = child.wait().expect("wait for busy relay");
    assert!(status.success(), "busy relay failed: {status}");
    let _ = fs::remove_file(received);
    let _ = fs::remove_dir(root.path());
    drop(guard);
}

#[cfg(unix)]
#[test]
fn detaching_one_client_leaves_another_client_attached() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut first = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start first stay attach");
    let mut second = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start second stay attach");

    wait_for_attached(&guard.tmux, &name, &mut first);
    wait_for_attached(&guard.tmux, &name, &mut second);
    for _ in 0..200 {
        if client_count(&guard.tmux, &name) == 2 {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client_count(&guard.tmux, &name), 2);

    first
        .stdin
        .as_mut()
        .expect("first stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send first stay detach key");
    let first_status = first.wait().expect("wait for first detached stay");
    assert!(
        first_status.success(),
        "first detach failed: {first_status}"
    );

    for _ in 0..200 {
        if client_count(&guard.tmux, &name) == 1 {
            break;
        }
        if let Some(status) = second.try_wait().expect("check second stay status") {
            panic!("second stay detached unexpectedly: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client_count(&guard.tmux, &name), 1);
    assert!(
        second
            .try_wait()
            .expect("check retained second stay status")
            .is_none(),
        "second stay was detached with the first client"
    );

    second
        .stdin
        .as_mut()
        .expect("second stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send second stay detach key");
    let second_status = second.wait().expect("wait for second detached stay");
    assert!(
        second_status.success(),
        "second detach failed: {second_status}"
    );
}

#[cfg(unix)]
#[test]
fn create_attach_reports_each_client_modifier_in_tmux_status() {
    let _lock = pty_test_lock();
    for (flags, label) in [
        ("", None),
        ("--read-only", Some("(view only)")),
        ("--low-priority", Some("(low priority)")),
        (
            "--read-only --low-priority",
            Some("(view only / low priority)"),
        ),
    ] {
        let name = unique_name();
        let namespace = unique_namespace();
        let guard = SessionGuard::empty(namespace.clone());
        let shim = TmuxShim::new();
        let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
        let command = format!(
            "stty rows 24 cols 80; exec {} create {} --attach {} -- /bin/sh -c {}",
            shell_quote(&executable.to_string_lossy()),
            shell_quote(&name),
            flags,
            shell_quote("sleep 30"),
        );
        let mut child = pty_shell_script(&command, &shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("TERM", "xterm-256color")
            .env("PATH", shim.path())
            .env("STAY_TEST_NAMESPACE", &namespace)
            .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
            .spawn()
            .expect("start create-and-attach test");

        wait_for_attached(&guard.tmux, &name, &mut child);
        if let Some(label) = label {
            wait_for_status_label(&guard.tmux, &name, &mut child, label);
        } else {
            wait_for_status_without_modifier_labels(&guard.tmux, &name, &mut child);
        }
        child
            .stdin
            .as_mut()
            .expect("create-and-attach stdin")
            .write_all(b"\x1c")
            .expect("detach create-and-attach test");
        assert!(
            child
                .wait()
                .expect("wait for create-and-attach test")
                .success()
        );
    }
}

#[cfg(unix)]
#[test]
fn preexisting_cli_attach_reapplies_builtin_status_for_each_modifier() {
    let _lock = pty_test_lock();
    for (flags, label) in [
        ("", None),
        ("--read-only", Some("(view only)")),
        ("--low-priority", Some("(low priority)")),
        (
            "--read-only --low-priority",
            Some("(view only / low priority)"),
        ),
    ] {
        let name = unique_name();
        let namespace = unique_namespace();
        let guard = SessionGuard::new(namespace.clone(), &name);
        for (option, value) in [
            ("status-left", "stale-left"),
            ("status-right", "stale-right"),
        ] {
            let status = guard
                .tmux
                .command(["set-option", "-g", option, value])
                .status()
                .expect("replace pre-existing status setting");
            assert!(status.success(), "failed to replace {option}");
        }

        let shim = TmuxShim::new();
        let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
        let command = format!(
            "stty rows 24 cols 80; exec {} attach {} {}",
            shell_quote(&executable.to_string_lossy()),
            shell_quote(&name),
            flags,
        );
        let mut child = pty_shell_script(&command, &shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("TERM", "xterm-256color")
            .env("PATH", shim.path())
            .env("STAY_TEST_NAMESPACE", &namespace)
            .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
            .spawn()
            .expect("start pre-existing CLI attach test");

        wait_for_attached(&guard.tmux, &name, &mut child);
        if let Some(label) = label {
            wait_for_status_label(&guard.tmux, &name, &mut child, label);
        } else {
            wait_for_status_without_modifier_labels(&guard.tmux, &name, &mut child);
        }
        child
            .stdin
            .as_mut()
            .expect("pre-existing attach stdin")
            .write_all(b"\x1c")
            .expect("detach pre-existing CLI attach test");
        assert!(
            child
                .wait()
                .expect("wait for pre-existing CLI attach")
                .success()
        );
    }
}

#[cfg(unix)]
#[test]
fn preexisting_attach_preserves_user_status_settings() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let status = guard
        .tmux
        .command(["set-option", "-g", "status-left", "user-left"])
        .status()
        .expect("set user status-left");
    assert!(status.success());
    let status = guard
        .tmux
        .command(["set-option", "-g", "status-right", "user-right"])
        .status()
        .expect("set user status-right");
    assert!(status.success());

    let home = TempPath::file("stay-user-home");
    fs::create_dir(&home).expect("create user home");
    fs::write(home.join(".tmux.conf"), "set -g status-left user-left\n")
        .expect("write user tmux config");
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {} attach {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("HOME", home.path())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start user-status attach test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    let status = guard
        .tmux
        .run(["show-options", "-gqv", "status-left"])
        .expect("read user status-left");
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "user-left");
    child
        .stdin
        .as_mut()
        .expect("user-status attach stdin")
        .write_all(b"\x1c")
        .expect("detach user-status attach test");
    assert!(child.wait().expect("wait for user-status attach").success());
    let _ = fs::remove_file(home.join(".tmux.conf"));
    let _ = fs::remove_dir(&home);
}

#[cfg(unix)]
#[test]
fn force_recreate_create_attach_returns_the_new_command_status() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::empty(namespace.clone());
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 1000,
        log_capture_interval_seconds: 5,
    };
    session::create_session_with_shell(
        &guard.tmux,
        &config,
        &name,
        None,
        &[
            "sh".to_owned(),
            "-c".to_owned(),
            "sleep 1; exit 5".to_owned(),
        ],
        std::path::Path::new("/bin/sh"),
        None,
    )
    .expect("create terminated session for force recreate");
    wait_for_terminated_session(&guard.tmux, &name, 5);

    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {} create {} --force-recreate --attach -- /bin/sh -c {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote("sleep 1; exit 9"),
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start force-recreate create-and-attach test");

    let status = wait_for_child_status(&mut child);
    assert_eq!(status.code(), Some(9), "unexpected attach status: {status}");
    wait_for_terminated_session(&guard.tmux, &name, 9);
}

#[cfg(unix)]
#[test]
fn attach_flushes_partial_prompts_before_input_and_after_commands() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "printf 'PROMPT>'; read value; printf 'OUT:%s\\nPROMPT>' \"$value\"; sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {} attach {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name)
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start partial-prompt test");

    let stdout = child.stdout.take().expect("partial-prompt stdout");
    let observed_output = Arc::new(Mutex::new(Vec::new()));
    let output_for_thread = Arc::clone(&observed_output);
    let output_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => output_for_thread
                    .lock()
                    .expect("lock partial-prompt output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read partial-prompt output: {error}"),
            }
        }
    });

    wait_for_output_occurrences(&observed_output, "PROMPT>", 1);
    child
        .stdin
        .as_mut()
        .expect("partial-prompt stdin")
        .write_all(b"hello\n")
        .expect("send command to partial-prompt session");
    wait_for_output_contains(&observed_output, "OUT:hello");
    wait_for_output_occurrences(&observed_output, "PROMPT>", 2);
    child
        .stdin
        .as_mut()
        .expect("partial-prompt stdin")
        .write_all(b"\x1c")
        .expect("detach partial-prompt session");

    let status = child.wait().expect("wait for partial-prompt test");
    output_thread
        .join()
        .expect("join partial-prompt output reader");
    assert!(status.success(), "partial-prompt attach failed: {status}");
    drop(guard);
}

#[cfg(unix)]
#[test]
fn normal_detach_restores_cooked_terminal_settings() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "{} attach {}; stty -a",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name)
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start cooked-terminal test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let result = child
        .wait_with_output()
        .expect("wait for cooked-terminal test");
    let status = result.status;
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(status.success(), "normal detach failed: {output}");
    assert!(
        output.contains("icanon"),
        "terminal remained non-canonical: {output}"
    );
    assert!(
        output.contains("echo"),
        "terminal echo was not restored: {output}"
    );
}

#[test]
fn bare_non_tty_requires_the_list_subcommand() {
    let namespace = unique_namespace();
    let shim = TmuxShim::new();
    let mut command = shim.stay_command();
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .output()
        .expect("run non-TTY picker boundary test");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("use `stay list`"));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn picker_renders_terminated_rows_when_focused_or_not() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let terminated_name = format!("dead-{}", unique_name());
    let live_name = format!("live-{}", unique_name());
    let terminated_guard = SessionGuard::new_with_command(
        namespace.clone(),
        &terminated_name,
        &["sh", "-c", "sleep 1; exit 7"],
    );
    let live_guard = SessionGuard::new(namespace.clone(), &live_name);
    wait_for_pane_exit_status(&terminated_guard.tmux, &terminated_name, 7);
    wait_for_terminated_session(&terminated_guard.tmux, &terminated_name, 7);

    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 120; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start terminated-session picker test");

    let stdout = child.stdout.take().expect("terminated picker stdout");
    let observed_output = Arc::new(Mutex::new(Vec::new()));
    let output_for_thread = Arc::clone(&observed_output);
    let output_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => output_for_thread
                    .lock()
                    .expect("lock terminated picker output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read terminated picker output: {error}"),
            }
        }
    });

    wait_for_output_contains(&observed_output, &terminated_name);
    wait_for_output_contains(&observed_output, "terminated");
    let initial_output = {
        let observed = observed_output.lock().expect("lock initial picker output");
        strip_csi_sequences(&observed)
    };
    assert!(
        initial_output.contains("[terminated exit=7 @"),
        "terminated suffix missing: {initial_output:?}"
    );
    child
        .stdin
        .as_mut()
        .expect("terminated picker stdin")
        .write_all(b"q")
        .expect("quit terminated picker test");
    let result = child
        .wait_with_output()
        .expect("wait for terminated picker test");
    output_thread
        .join()
        .expect("join terminated picker output reader");
    assert!(result.status.success(), "terminated picker failed");
    drop(live_guard);
    drop(terminated_guard);
}

#[cfg(unix)]
#[test]
fn empty_picker_opens_the_focused_create_row() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start empty picker test");

    let stdout = child.stdout.take().expect("empty picker stdout");
    let observed_output = Arc::new(Mutex::new(Vec::new()));
    let output_for_thread = Arc::clone(&observed_output);
    let output_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => output_for_thread
                    .lock()
                    .expect("lock empty picker output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read empty picker output: {error}"),
            }
        }
    });
    wait_for_output_contains(&observed_output, "create new session");
    wait_for_output_contains(&observed_output, "c create");
    wait_for_output_contains(&observed_output, "Enter attach");
    wait_for_output_contains(&observed_output, "q/Esc quit");
    wait_for_output_contains(&observed_output, "Esc");
    wait_for_output_contains(&observed_output, "quit");

    let stdin = child.stdin.as_mut().expect("empty picker stdin");
    stdin.write_all(b"\r").expect("press Enter in empty picker");
    wait_for_output_contains(&observed_output, "New session");
    stdin
        .write_all(b"evl\x1b")
        .expect("edit and leave the empty create prompt");
    thread::sleep(Duration::from_millis(150));

    child
        .stdin
        .as_mut()
        .expect("empty picker stdin")
        .write_all(b"q")
        .expect("quit empty picker");
    let result = child.wait_with_output().expect("wait for empty picker");
    output_thread
        .join()
        .expect("join empty picker output reader");
    assert!(result.status.success(), "empty picker failed");
}

#[cfg(unix)]
#[test]
fn picker_recreation_requires_confirmation_for_live_and_terminated_sessions() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let terminated_name = format!("dead-{}", unique_name());
    let live_name = format!("live-{}", unique_name());
    let terminated_guard = SessionGuard::new_with_command(
        namespace.clone(),
        &terminated_name,
        &["sh", "-c", "sleep 1; exit 7"],
    );
    let live_guard = SessionGuard::new(namespace.clone(), &live_name);
    wait_for_pane_exit_status(&terminated_guard.tmux, &terminated_name, 7);
    wait_for_terminated_session(&terminated_guard.tmux, &terminated_name, 7);

    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 120; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start recreation picker test");
    let (observed_output, output_thread) = start_output_reader(&mut child, "recreation picker");
    wait_for_output_contains(&observed_output, &live_name);
    wait_for_output_contains(&observed_output, &terminated_name);

    // The create row is initially selected; two Down keys select the live row
    // after the alphabetically earlier terminated row.
    let live_count =
        String::from_utf8_lossy(&observed_output.lock().expect("lock live recreation output"))
            .matches(live_name.as_str())
            .count();
    write_picker_input(&mut child, b"\x1b[B\x1b[Br");
    wait_for_output_occurrences_after(&observed_output, &live_name, live_count);
    write_picker_input(&mut child, b"n");
    thread::sleep(Duration::from_millis(150));
    assert!(
        live_guard
            .tmux
            .list_sessions()
            .expect("list live session after cancellation")
            .iter()
            .any(|session| session.name == live_name && !session.terminated)
    );

    // Move back to the terminated row and verify it is protected by the same
    // confirmation flow.
    let terminated_count = String::from_utf8_lossy(
        &observed_output
            .lock()
            .expect("lock terminated recreation output"),
    )
    .matches(terminated_name.as_str())
    .count();
    write_picker_input(&mut child, b"\x1b[Ar");
    wait_for_output_occurrences_after(&observed_output, &terminated_name, terminated_count);
    write_picker_input(&mut child, b"nq");
    let result = child
        .wait_with_output()
        .expect("wait for recreation picker");
    output_thread
        .join()
        .expect("join recreation picker output reader");
    assert!(result.status.success(), "recreation picker failed");
    assert!(
        terminated_guard
            .tmux
            .list_sessions()
            .expect("list terminated session after cancellation")
            .iter()
            .any(|session| session.name == terminated_name && session.terminated)
    );
}

#[cfg(unix)]
#[test]
fn picker_quit_restores_the_outer_terminal() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; {}; stty -a",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker terminal test");

    let (observed_output, output_thread) = start_output_reader(&mut child, "picker terminal");
    wait_for_output_contains(&observed_output, "select");
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit picker");
    let result = child.wait().expect("wait for picker terminal test");
    output_thread
        .join()
        .expect("join picker terminal output reader");
    let observed = observed_output.lock().expect("lock picker terminal output");
    let output = String::from_utf8_lossy(&observed).into_owned();
    assert!(result.success(), "picker quit failed: {output}");
    assert!(output.contains("icanon"), "terminal remained raw: {output}");
    assert!(
        output.contains("echo"),
        "terminal echo was not restored: {output}"
    );
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_returns_to_the_picker_after_attach_failure() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create picker attach-failure directory");
    let failure_marker = root.join("fail-attach");
    fs::write(&failure_marker, "fail").expect("enable picker attach failure");
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_FAIL_ATTACH_FILE", &failure_marker)
        .spawn()
        .expect("start picker attach-failure test");

    let stdout = child.stdout.take().expect("picker attach-failure stdout");
    let observed_output = Arc::new(Mutex::new(Vec::new()));
    let output_for_thread = Arc::clone(&observed_output);
    let output_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => output_for_thread
                    .lock()
                    .expect("lock picker attach-failure output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read picker attach-failure output: {error}"),
            }
        }
    });

    wait_for_output_contains(&observed_output, &name);
    child
        .stdin
        .as_mut()
        .expect("picker attach-failure stdin")
        .write_all(b"\x1b[B")
        .expect("select the picker session");
    thread::sleep(Duration::from_millis(50));
    let status = guard
        .tmux
        .command(["kill-session", "-t", &name])
        .status()
        .expect("kill selected picker session");
    assert!(status.success(), "kill selected picker session failed");
    let recovery_output_start = observed_output
        .lock()
        .expect("lock picker attach-failure output before recovery")
        .len();
    child
        .stdin
        .as_mut()
        .expect("picker attach-failure stdin")
        .write_all(b"\r")
        .expect("press Enter after selecting the session");

    wait_for_output_contains(&observed_output, "tmux");
    let recovery_output = output_since(&observed_output, recovery_output_start);
    assert!(
        !recovery_output.contains(&name),
        "recovered picker still listed the killed session: {recovery_output:?}"
    );
    assert!(
        child
            .try_wait()
            .expect("check picker after attach failure")
            .is_none(),
        "attach failure exited the picker"
    );
    child
        .stdin
        .as_mut()
        .expect("picker attach-failure stdin")
        .write_all(b"q")
        .expect("quit picker after attach failure");
    let result = child
        .wait_with_output()
        .expect("wait for picker attach-failure test");
    output_thread
        .join()
        .expect("join picker attach-failure output reader");
    assert!(
        result.status.success(),
        "picker attach-failure test failed: {:?}",
        result.status
    );
    let _ = fs::remove_file(failure_marker);
    let _ = fs::remove_dir(root.path());
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_sigterm_restores_cooked_terminal_settings() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create picker SIGTERM test directory");
    let pid_path = root.join("stay.pid");
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "{} & echo $! > {}; wait $!; stty -a",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&pid_path.to_string_lossy())
    );
    let child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker SIGTERM test");

    let pid = wait_for_nonempty_file(&pid_path)
        .trim()
        .parse::<i32>()
        .expect("parse picker PID");
    kill(Pid::from_raw(pid), Signal::SIGTERM).expect("send SIGTERM to picker");
    let result = child
        .wait_with_output()
        .expect("wait for picker SIGTERM test");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.status.success(), "picker SIGTERM failed: {output}");
    assert!(
        output.contains("icanon"),
        "terminal remained non-canonical: {output}"
    );
    assert!(
        output.contains("echo"),
        "terminal echo was not restored: {output}"
    );
    let _ = fs::remove_file(pid_path);
    let _ = fs::remove_dir(root.path());
}

#[cfg(unix)]
#[test]
fn picker_create_creates_and_attaches_the_named_session() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let name = format!("create-{}", unique_name());
    let guard = SessionGuard::empty(namespace.clone());
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker create test");

    let (observed_output, output_thread) = start_output_reader(&mut child, "picker create");
    wait_for_output_contains(&observed_output, "create");
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(format!("c{name}\r").as_bytes())
        .expect("create picker session");
    wait_for_attached(&guard.tmux, &name, &mut child);
    wait_for_status_without_modifier_labels(&guard.tmux, &name, &mut child);
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1c")
        .expect("detach created picker session");
    let previous_render_count =
        String::from_utf8_lossy(&observed_output.lock().expect("lock picker create output"))
            .matches("create")
            .count();
    wait_for_output_occurrences_after(&observed_output, "create", previous_render_count);
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit returned picker");
    assert!(child.wait().expect("wait for picker create test").success());
    output_thread
        .join()
        .expect("join picker create output reader");
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("list created picker session")
            .iter()
            .any(|session| session.name == name)
    );
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_returns_after_detach_and_can_attach_again_on_both_screen_preferences() {
    let _lock = pty_test_lock();
    for no_alt_screen in [false, true] {
        let namespace = unique_namespace();
        let first_name = format!("first-{}", unique_name());
        let second_name = format!("second-{}", unique_name());
        let guard = SessionGuard::empty(namespace.clone());
        for name in [&first_name, &second_name] {
            let status = guard
                .tmux
                .command(["new-session", "-d", "-s", name, "--", "sleep", "30"])
                .status()
                .expect("create picker reattach session");
            assert!(status.success(), "tmux failed to create {name}");
            let status = guard
                .tmux
                .command(["set-option", "-t", name, "remain-on-exit", "on"])
                .status()
                .expect("retain picker reattach session");
            assert!(status.success(), "tmux failed to retain {name}");
        }

        let shim = TmuxShim::new();
        let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
        let screen_flag = if no_alt_screen {
            " --no-alt-screen"
        } else {
            ""
        };
        let command = format!(
            "stty rows 24 cols 100; exec {}{}",
            shell_quote(&executable.to_string_lossy()),
            screen_flag
        );
        let mut child = pty_shell_script(&command, &shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("TERM", "xterm-256color")
            .env("PATH", shim.path())
            .env("STAY_TEST_NAMESPACE", &namespace)
            .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
            .spawn()
            .expect("start picker reattach test");

        let stdout = child.stdout.take().expect("picker reattach stdout");
        let observed_output = Arc::new(Mutex::new(Vec::new()));
        let output_for_thread = Arc::clone(&observed_output);
        let output_thread = thread::spawn(move || {
            let mut stdout = stdout;
            let mut bytes = [0_u8; 4096];
            loop {
                match stdout.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(length) => output_for_thread
                        .lock()
                        .expect("lock picker reattach output")
                        .extend_from_slice(&bytes[..length]),
                    Err(error) => panic!("read picker reattach output: {error}"),
                }
            }
        });

        wait_for_output_contains(&observed_output, &first_name);
        let title_count = {
            let observed = observed_output.lock().expect("lock initial picker output");
            String::from_utf8_lossy(&observed).matches("stay v").count()
        };
        write_picker_input(&mut child, b"\x1b[B\r");
        wait_for_attached(&guard.tmux, &first_name, &mut child);
        write_picker_input(&mut child, b"\x1c");
        wait_for_output_occurrences_after(&observed_output, "stay v", title_count);

        write_picker_input(&mut child, b"\x1b[B\x1b[B\r");
        wait_for_attached(&guard.tmux, &second_name, &mut child);
        let title_count = {
            let observed = observed_output.lock().expect("lock second picker output");
            String::from_utf8_lossy(&observed).matches("stay v").count()
        };
        write_picker_input(&mut child, b"\x1c");
        wait_for_output_occurrences_after(&observed_output, "stay v", title_count);
        write_picker_input(&mut child, b"q");
        assert!(
            child
                .wait()
                .expect("wait for picker reattach test")
                .success(),
            "picker reattach test failed"
        );
        output_thread
            .join()
            .expect("join picker reattach output reader");
        drop(guard);
    }
}

#[cfg(unix)]
#[test]
fn picker_navigation_keys_select_expected_rows_in_a_pty() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let guard = SessionGuard::empty(namespace.clone());
    let names = ["nav-a", "nav-b", "nav-c", "nav-d", "nav-e", "nav-f"];
    for name in names {
        let status = guard
            .tmux
            .command(["new-session", "-d", "-s", name, "--", "sleep", "30"])
            .status()
            .expect("create picker navigation session");
        assert!(status.success(), "tmux failed to create {name}");
    }

    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 8 cols 100; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker navigation test");

    let stdout = child.stdout.take().expect("picker navigation stdout");
    let observed_output = Arc::new(Mutex::new(Vec::new()));
    let output_for_thread = Arc::clone(&observed_output);
    let output_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => output_for_thread
                    .lock()
                    .expect("lock picker navigation output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read picker navigation output: {error}"),
            }
        }
    });

    wait_for_output_contains(&observed_output, names[0]);
    let title_count = {
        let observed = observed_output
            .lock()
            .expect("lock initial navigation output");
        String::from_utf8_lossy(&observed).matches("stay v").count()
    };
    write_picker_input(&mut child, b"\x1b[6~\r");
    wait_for_attached(&guard.tmux, names[2], &mut child);
    write_picker_input(&mut child, b"\x1c");
    wait_for_output_occurrences_after(&observed_output, "stay v", title_count);

    write_picker_input(&mut child, b"\x1b[H\x1b[B\r");
    wait_for_attached(&guard.tmux, names[0], &mut child);
    write_picker_input(&mut child, b"\x1c");
    wait_for_output_occurrences_after(&observed_output, "stay v", title_count + 1);

    write_picker_input(&mut child, b"\x1b[F\r");
    wait_for_attached(&guard.tmux, names[5], &mut child);
    write_picker_input(&mut child, b"\x1c");
    wait_for_output_occurrences_after(&observed_output, "stay v", title_count + 2);
    write_picker_input(&mut child, b"q");
    assert!(
        child
            .wait()
            .expect("wait for picker navigation test")
            .success(),
        "picker navigation test failed"
    );
    output_thread
        .join()
        .expect("join picker navigation output reader");
    drop(guard);
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn picker_attachment_status_covers_auto_and_forced_main_screen() {
    let _lock = pty_test_lock();
    for (no_alt_screen, modifiers, label) in [
        (false, b"\r".as_slice(), None),
        (false, b"v\r".as_slice(), Some("(view only)")),
        (true, b"l\r".as_slice(), Some("(low priority)")),
        (
            false,
            b"vl\r".as_slice(),
            Some("(view only / low priority)"),
        ),
    ] {
        let namespace = unique_namespace();
        let name = unique_name();
        let guard = SessionGuard::empty(namespace.clone());
        let config = Config {
            default_command: Some("ignored".to_owned()),
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 1000,
            log_capture_interval_seconds: 5,
        };
        session::create_session_with_shell(
            &guard.tmux,
            &config,
            &name,
            None,
            &["sleep".to_owned(), "30".to_owned()],
            std::path::Path::new("/bin/sh"),
            None,
        )
        .expect("create picker status session");
        for (option, value) in [
            ("status-left", "stale-left"),
            ("status-right", "stale-right"),
        ] {
            let status = guard
                .tmux
                .command(["set-option", "-g", option, value])
                .status()
                .expect("replace pre-existing status setting");
            assert!(status.success(), "failed to replace {option}");
        }

        let shim = TmuxShim::new();
        let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
        let screen_flag = if no_alt_screen {
            " --no-alt-screen"
        } else {
            ""
        };
        let command = format!(
            "stty rows 24 cols 80; exec {}{}",
            shell_quote(&executable.to_string_lossy()),
            screen_flag
        );
        let mut child = pty_shell_script(&command, &shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("TERM", "xterm-256color")
            .env("PATH", shim.path())
            .env("STAY_TEST_NAMESPACE", &namespace)
            .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
            .spawn()
            .expect("start picker status test");

        let (observed_output, output_thread) = start_output_reader(&mut child, "picker status");
        wait_for_output_contains(&observed_output, &name);
        let mut picker_input = vec![0x1b, b'[', b'B'];
        picker_input.extend_from_slice(modifiers);
        child
            .stdin
            .as_mut()
            .expect("picker stdin")
            .write_all(&picker_input)
            .expect("attach with picker modifier");
        wait_for_attached(&guard.tmux, &name, &mut child);
        if let Some(label) = label {
            wait_for_status_label(&guard.tmux, &name, &mut child, label);
        } else {
            wait_for_status_without_modifier_labels(&guard.tmux, &name, &mut child);
        }
        child
            .stdin
            .as_mut()
            .expect("picker relay stdin")
            .write_all(b"\x1c")
            .expect("detach picker status test");
        let rendered_before_return =
            String::from_utf8_lossy(&observed_output.lock().expect("lock picker status output"))
                .matches(&name)
                .count();
        wait_for_output_occurrences_after(&observed_output, &name, rendered_before_return);
        child
            .stdin
            .as_mut()
            .expect("picker stdin")
            .write_all(b"q")
            .expect("quit returned picker");
        assert!(child.wait().expect("wait for picker status test").success());
        output_thread
            .join()
            .expect("join picker status output reader");
        drop(guard);
    }
}

#[cfg(unix)]
#[test]
fn picker_kill_confirmation_supports_safe_cancel_and_yes_paths() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let name = format!("kill-{}", unique_name());
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker kill test");

    let (observed_output, output_thread) = start_output_reader(&mut child, "picker kill");
    wait_for_output_contains(&observed_output, &name);
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1b[Bkn")
        .expect("cancel picker kill with direct no");
    let rendered_before_cancel =
        String::from_utf8_lossy(&observed_output.lock().expect("lock picker kill output"))
            .matches(&name)
            .count();
    wait_for_output_occurrences_after(&observed_output, &name, rendered_before_cancel);
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("list after direct-no confirmation")
            .iter()
            .any(|session| session.name == name)
    );

    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"k\x1b")
        .expect("cancel picker kill");
    let rendered_before_second_cancel =
        String::from_utf8_lossy(&observed_output.lock().expect("lock picker kill output"))
            .matches(&name)
            .count();
    wait_for_output_occurrences_after(&observed_output, &name, rendered_before_second_cancel);
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("list after cancelled confirmation")
            .iter()
            .any(|session| session.name == name)
    );

    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"k\x1b[D\r")
        .expect("confirm picker kill with yes");
    wait_for_output_contains(&observed_output, "create new session");
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit after picker kill");
    assert!(child.wait().expect("wait for picker kill test").success());
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("list after picker kill")
            .is_empty()
    );
    output_thread
        .join()
        .expect("join picker kill output reader");
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_forwards_typed_ahead_input_to_the_attached_session() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create picker handoff directory");
    let marker = root.join("picker-input.txt");
    let command = format!(
        "IFS= read -r value; printf '%s' \"$value\" > {}; sleep 30",
        shell_quote(&marker.to_string_lossy())
    );
    let command_words = ["sh", "-c", command.as_str()];
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &command_words);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker handoff test");

    let (observed_output, output_thread) = start_output_reader(&mut child, "picker handoff");
    wait_for_output_contains(&observed_output, &name);
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1b[B\rtyped-ahead\n")
        .expect("send picker selection and typed-ahead input");
    wait_for_file_contents(&marker, "typed-ahead");
    wait_for_attached(&guard.tmux, &name, &mut child);
    child
        .stdin
        .as_mut()
        .expect("relay stdin")
        .write_all(b"\x1c")
        .expect("detach after picker handoff");
    let rendered_before_return =
        String::from_utf8_lossy(&observed_output.lock().expect("lock picker handoff output"))
            .matches(&name)
            .count();
    wait_for_output_occurrences_after(&observed_output, &name, rendered_before_return);
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit returned picker");
    assert!(child.wait().expect("wait for picker handoff").success());
    output_thread
        .join()
        .expect("join picker handoff output reader");
    let _ = fs::remove_file(marker);
    let _ = fs::remove_dir(root.path());
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_clears_selection_when_the_selected_session_disappears() {
    let _lock = pty_test_lock();
    let namespace = unique_namespace();
    let first = format!("a-{}", unique_name());
    let second = format!("b-{}", unique_name());
    let guard = SessionGuard::new(namespace.clone(), &first);
    let status = guard
        .tmux
        .command(["new-session", "-d", "-s", &second, "--", "sleep", "30"])
        .status()
        .expect("create second picker session");
    assert!(status.success());
    let status = guard
        .tmux
        .command(["set-option", "-t", &second, "remain-on-exit", "on"])
        .status()
        .expect("retain second picker session");
    assert!(status.success());

    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker identity test");

    thread::sleep(Duration::from_millis(800));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1b[B")
        .expect("select first picker session");
    guard
        .tmux
        .command(["kill-session", "-t", &first])
        .status()
        .expect("kill selected picker session");
    thread::sleep(Duration::from_millis(800));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\r")
        .expect("press attach after selection disappeared");
    thread::sleep(Duration::from_millis(200));
    let sessions = guard.tmux.list_sessions().expect("list remaining session");
    assert!(sessions.iter().all(|session| !session.attached));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1bq")
        .expect("quit picker identity test");
    assert!(
        child
            .wait()
            .expect("wait for picker identity test")
            .success()
    );
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_retains_its_last_list_when_a_poll_fails() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create picker poll directory");
    let failure_marker = root.join("fail-list");
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .env("STAY_TEST_FAIL_LIST_FILE", &failure_marker)
        .spawn()
        .expect("start picker poll test");

    let stdout = child.stdout.take().expect("picker stdout");
    let observed_output = Arc::new(Mutex::new(Vec::new()));
    let output_for_thread = Arc::clone(&observed_output);
    let output_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = [0_u8; 4096];
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(length) => output_for_thread
                    .lock()
                    .expect("lock picker output")
                    .extend_from_slice(&bytes[..length]),
                Err(error) => panic!("read picker output: {error}"),
            }
        }
    });
    wait_for_output_contains(&observed_output, &name);
    fs::write(&failure_marker, "fail").expect("enable picker poll failure");
    thread::sleep(Duration::from_millis(800));
    assert!(
        child
            .try_wait()
            .expect("check picker after poll failure")
            .is_none(),
        "poll failure exited the picker"
    );
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit after poll failure");
    let result = child.wait_with_output().expect("wait for picker poll test");
    output_thread.join().expect("join picker output reader");
    let observed_output = observed_output.lock().expect("lock picker output");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&observed_output),
        String::from_utf8_lossy(&result.stderr)
    );
    let rendered_output = strip_csi_sequences(output.as_bytes());
    assert!(result.status.success(), "picker poll test failed: {output}");
    assert!(
        rendered_output.contains(&name),
        "last list was not retained: {output}"
    );
    assert!(
        rendered_output.contains("picker poll fail"),
        "poll error was not rendered: {output}"
    );
    let _ = fs::remove_file(failure_marker);
    let _ = fs::remove_dir(root.path());
    drop(guard);
}

#[cfg(unix)]
#[test]
fn sigterm_detaches_and_restores_cooked_terminal_settings() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create SIGTERM test directory");
    let pid_path = root.join("stay.pid");
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "{} attach {} & echo $! > {}; wait $!; stty -a",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&pid_path.to_string_lossy())
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start SIGTERM test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    let pid = wait_for_nonempty_file(&pid_path)
        .trim()
        .parse::<i32>()
        .expect("parse stay PID");
    kill(Pid::from_raw(pid), Signal::SIGTERM).expect("send SIGTERM to stay");
    for _ in 0..100 {
        if child.try_wait().expect("check SIGTERM test").is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let result = child.wait_with_output().expect("wait for SIGTERM test");
    let status = result.status;
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(status.success(), "SIGTERM detach failed: {output}");
    assert!(
        output.contains("icanon"),
        "terminal remained non-canonical: {output}"
    );
    assert!(
        output.contains("echo"),
        "terminal echo was not restored: {output}"
    );
    let _ = fs::remove_file(pid_path);
    let _ = fs::remove_dir(root.path());
}

#[cfg(unix)]
#[test]
fn copy_mode_key_enters_tmux_copy_mode_without_forwarding() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
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
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\0")
        .expect("send copy-mode key");
    for _ in 0..100 {
        let output = guard
            .tmux
            .command([
                "list-panes",
                "-t",
                &format!("{name}:0"),
                "-F",
                "#{pane_in_mode}",
            ])
            .output()
            .expect("query copy mode");
        if String::from_utf8_lossy(&output.stdout).trim() == "1" {
            child
                .stdin
                .as_mut()
                .expect("stay PTY stdin")
                .write_all(b"\x1c")
                .expect("send stay detach key");
            let status = child.wait().expect("wait for copy-mode detach");
            assert!(status.success(), "copy-mode detach failed: {status}");
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("stay did not enter tmux copy mode");
}

#[cfg(unix)]
#[test]
fn forwards_ordinary_input_bytes_verbatim() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create forwarding test directory");
    let marker = root.join("input.txt");
    let script = root.join("reader.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nIFS= read -r value\nprintf '%s' \"$value\" > {}\nsleep 30\n",
            marker.display()
        ),
    )
    .expect("write forwarding test command");
    set_executable(&script);
    let command = script.to_string_lossy().into_owned();
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &[&command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start forwarding stay");

    wait_for_attached(&guard.tmux, &name, &mut child);
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"forwarded input\n")
        .expect("send ordinary input");
    wait_for_file_contents(&marker, "forwarded input");
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    assert!(child.wait().expect("wait for forwarding stay").success());
    let _ = fs::remove_file(script);
    let _ = fs::remove_file(marker);
    let _ = fs::remove_dir(root.path());
}

#[cfg(unix)]
#[test]
fn read_only_attach_does_not_forward_input_to_the_pane() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create read-only test directory");
    let marker = root.join("input.txt");
    let script = root.join("reader.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nIFS= read -r value\nprintf '%s' \"$value\" > {}\nsleep 30\n",
            marker.display()
        ),
    )
    .expect("write read-only test command");
    set_executable(&script);
    let command = script.to_string_lossy().into_owned();
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &[&command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let attach_command = format!(
        "{} attach {} -r",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name)
    );
    let mut child = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start read-only stay");

    wait_for_attached(&guard.tmux, &name, &mut child);
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"should not reach the pane\n")
        .expect("send read-only input");
    thread::sleep(Duration::from_millis(500));
    assert!(
        !marker.exists() || fs::read_to_string(&marker).expect("read marker").is_empty(),
        "read-only attach forwarded input to the pane"
    );

    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for read-only stay");
    assert!(status.success(), "read-only detach failed: {status}");
    assert!(
        !marker.exists() || fs::read_to_string(&marker).expect("read marker").is_empty(),
        "read-only attach forwarded input to the pane before detach completed"
    );
    let _ = fs::remove_file(&script);
    let _ = fs::remove_file(&marker);
    let _ = fs::remove_dir(&root);
}

#[cfg(unix)]
#[test]
fn forwards_attach_pty_output_to_stay_stdout() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new_with_command(
        namespace.clone(),
        &name,
        &["sh", "-c", "printf 'relay-output-marker\\n'; sleep 30"],
    );
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start output-forwarding stay");
    let mut stdout = child.stdout.take().expect("stay stdout pipe");
    let output_thread = thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .read_to_end(&mut output)
            .expect("read forwarded stay output");
        output
    });

    wait_for_attached(&guard.tmux, &name, &mut child);
    thread::sleep(Duration::from_millis(250));
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for output-forwarding stay");
    let output_bytes = output_thread.join().expect("join output reader");
    let output = String::from_utf8_lossy(&output_bytes);
    assert!(
        status.success(),
        "output forwarding detach failed: {status}"
    );
    assert!(
        output.contains("relay-output-marker"),
        "attach PTY output was not forwarded: {output:?}"
    );
}

#[cfg(unix)]
#[test]
fn returns_a_dead_panes_exit_status_after_detach() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard =
        SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", "sleep 1; exit 7"]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start stay for dead pane");

    let mut attached = false;
    for _ in 0..200 {
        if guard
            .tmux
            .list_sessions()
            .expect("list dead attach session")
            .iter()
            .any(|session| session.name == name && session.attached)
        {
            attached = true;
            break;
        }
        if let Some(status) = child.try_wait().expect("check dead attach status") {
            assert_eq!(status.code(), Some(7), "unexpected stay status: {status}");
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(attached, "stay did not attach to the dead-pane fixture");
    thread::sleep(Duration::from_millis(1200));
    if let Some(status) = child.try_wait().expect("check retained dead-pane status") {
        assert_eq!(status.code(), Some(7), "unexpected stay status: {status}");
        return;
    }
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for dead pane detach");
    assert_eq!(status.code(), Some(7), "unexpected stay status: {status}");
}

#[cfg(unix)]
#[test]
fn auto_detaches_when_the_attached_command_ends_and_preserves_the_session() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard =
        SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", "sleep 1; exit 7"]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start automatic-detach test");

    let status = wait_for_child_status(&mut child);
    assert_eq!(status.code(), Some(7), "unexpected stay status: {status}");
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("list retained terminated session")
            .iter()
            .any(|session| session.name == name && !session.attached)
    );
    assert_eq!(guard.tmux.pane_exit_status(&name).unwrap(), Some(7));
}

#[cfg(unix)]
#[test]
fn a_signal_killed_pane_auto_detaches_and_reports_128_plus_the_signal() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", "sleep 30"]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "{} attach {}; echo \"stay exit status: $?\"; stty -a",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name)
    );
    let mut child = pty_shell_script(&command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start signal-killed pane test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    kill(pane_pid(&guard.tmux, &name), Signal::SIGKILL).expect("signal the attached pane");

    let result = child
        .wait_with_output()
        .expect("wait for signal-killed pane test");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.status.success(),
        "wrapper shell script failed: {output}"
    );
    assert!(
        output.contains("stay exit status: 137"),
        "expected exit status 137: {output}"
    );
    assert!(
        !output.to_lowercase().contains("error"),
        "expected no error output: {output}"
    );
    assert!(
        output.contains("icanon"),
        "terminal remained non-canonical: {output}"
    );
    assert!(
        output.contains("echo"),
        "terminal echo was not restored: {output}"
    );
}

#[cfg(unix)]
#[test]
fn postmortem_attach_waits_for_manual_detach_and_exits_zero() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard =
        SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", "sleep 1; exit 5"]);
    wait_for_pane_exit_status(&guard.tmux, &name, 5);
    thread::sleep(Duration::from_millis(1200));

    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start postmortem attach test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    thread::sleep(Duration::from_millis(800));
    assert!(
        child
            .try_wait()
            .expect("check postmortem stay status")
            .is_none(),
        "postmortem attach auto-detached"
    );
    child
        .stdin
        .as_mut()
        .expect("postmortem stay stdin")
        .write_all(b"\x1c")
        .expect("detach postmortem attach");
    let status = child.wait().expect("wait for postmortem attach");
    assert_eq!(status.code(), Some(0), "unexpected stay status: {status}");
}

#[cfg(unix)]
#[test]
fn manual_detach_after_command_end_still_propagates_status() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = TempPath::file(unique_name());
    fs::create_dir(&root).expect("create manual-detach test directory");
    let marker = root.join("ended");
    let command = format!(
        "sleep 1; printf done > {}; exit 9",
        shell_quote(&marker.to_string_lossy())
    );
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start manual-detach race test");

    wait_for_attached(&guard.tmux, &name, &mut child);
    wait_for_file_contents(&marker, "done");
    if child
        .try_wait()
        .expect("check manual-detach stay status")
        .is_none()
    {
        child
            .stdin
            .as_mut()
            .expect("manual-detach stay stdin")
            .write_all(b"\x1c")
            .expect("detach after command end");
    }
    let status = child.wait().expect("wait for manual-detach stay");
    assert_eq!(status.code(), Some(9), "unexpected stay status: {status}");
    let _ = fs::remove_file(marker);
    let _ = fs::remove_dir(root.path());
}

#[cfg(unix)]
#[test]
fn redirected_stdin_still_uses_the_attach_pty() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let mut command = shim.stay_command();
    let output = command
        .args(["attach", name.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start redirected stay");
    let mut child = output;
    wait_for_attached(&guard.tmux, &name, &mut child);
    guard
        .tmux
        .command(["kill-session", "-t", &name])
        .status()
        .expect("kill redirected test session");
    let status = child.wait().expect("wait for redirected stay");
    assert!(status.success(), "redirected stay failed: {status}");
}

#[test]
fn rejects_trailing_words_for_an_existing_session_without_attaching() {
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let mut command = shim.stay_command();
    let output = command
        .args(["attach", name.as_str(), "echo", "ignored"])
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .output()
        .expect("run stay with trailing command words");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("too many arguments"),
        "{stderr}"
    );
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("list existing session")
            .iter()
            .any(|session| session.name == name && !session.attached)
    );
}

fn wait_for_file_containing(path: &std::path::Path, expected: &str) -> String {
    for _ in 0..500 {
        if let Ok(contents) = fs::read_to_string(path)
            && contents.contains(expected)
        {
            return contents;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "timed out waiting for {} to contain {expected:?}",
        path.display()
    );
}

fn offset_sidecar_path(log_path: &std::path::Path) -> std::path::PathBuf {
    let mut name = log_path.as_os_str().to_owned();
    name.push(".offset");
    std::path::PathBuf::from(name)
}

fn scroll_filler(prefix: &str, count: usize) -> String {
    use std::fmt::Write as _;
    (0..count).fold(String::new(), |mut acc, index| {
        let _ = write!(acc, "echo {prefix}-{index}; ");
        acc
    })
}

/// Like [`scroll_filler`], but each line is exactly 79 columns, so a modest
/// line count crosses the 64 KiB OS pipe capacity that a deadlocked
/// `Tmux::run` would choke on (TASK-054). One `awk` invocation generates
/// every line, keeping the command short regardless of `count`; the fill
/// is a non-space character because `capture-pane` trims trailing
/// whitespace from every line, which would otherwise shrink the capture
/// back under 79 columns.
fn wide_scroll_filler(prefix: &str, count: usize) -> String {
    format!(
        "awk 'BEGIN{{for(i=0;i<{count};i++){{s=sprintf(\"{prefix}-%04d-\",i); \
         while(length(s)<79)s=s \"x\"; print s}}}}'; "
    )
}

#[cfg(unix)]
#[test]
fn default_log_mode_produces_a_clean_text_log_with_no_ansi() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = format!(
        "printf '\\033[31mred marker\\033[0m\\n'; {}sleep 30",
        scroll_filler("filler", 600)
    );
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-log");
    let attach_command = format!(
        "{} attach {} -l {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&log_path.to_string_lossy()),
    );
    let mut child = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start logged stay");

    wait_for_attached(&guard.tmux, &name, &mut child);
    let contents = wait_for_file_containing(&log_path, "red marker");
    assert!(
        !contents.contains('\u{1b}'),
        "log contains ANSI: {contents:?}"
    );
    let history_limit = guard
        .tmux
        .command(["show-options", "-t", &name, "history-limit"])
        .output()
        .expect("query history-limit");
    assert!(
        String::from_utf8_lossy(&history_limit.stdout).contains("50000"),
        "default logging did not raise history-limit"
    );

    // Sending the detach byte (Ctrl-\) before the relay has put the
    // terminal in raw mode lets the kernel's cooked-mode line discipline
    // treat it as SIGQUIT instead of stay's own detach key, killing the
    // process (status 131) instead of detaching cleanly. wait_for_attached
    // already implies the attach is well underway; this settles the last
    // sliver of that race.
    thread::sleep(Duration::from_millis(200));
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for logged stay");
    assert!(status.success(), "logged attach detach failed: {status}");
    let _ = fs::remove_file(&log_path);
    let _ = fs::remove_file(offset_sidecar_path(&log_path));
}

#[cfg(unix)]
#[test]
fn attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    // 2000 lines of 79 columns is ~160 KB of retained scrollback, well
    // past the 64 KiB OS pipe capacity that `Tmux::run` must drain
    // concurrently with the wait, rather than after it, or the `--raw`
    // backfill's `capture-pane` call times out and aborts the attach
    // before the relay loop ever starts (TASK-054). `--raw`'s backfill is
    // one atomic capture done once at attach start (unlike default mode's
    // per-tick incremental capture), which keeps this regression test
    // focused on `Tmux::run` alone.
    let command = format!(
        "printf '%-79s\\n' oldest-marker; {}sleep 30",
        wide_scroll_filler("filler", 2000)
    );
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let raised = guard
        .tmux
        .command(["set-option", "-t", &name, "history-limit", "6000"])
        .status()
        .expect("raise history-limit before the pane fills it");
    assert!(raised.success());
    // Wait for tmux's own pane processing (not just the writer) to catch
    // up with the whole flood before attaching, so the `--raw` backfill's
    // one-shot capture is never a race against tmux still ingesting it.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = guard
            .tmux
            .run(["capture-pane", "-p", "-t", &name, "-S", "-", "-E", "-"])
            .expect("poll pane for flood completion");
        if output
            .stdout
            .windows(11)
            .any(|window| window == b"filler-1999")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pane never produced its last line"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-log");
    let attach_command = format!(
        "{} attach {} -l {} --raw",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&log_path.to_string_lossy()),
    );
    let mut child = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start logged stay");

    // `LogSession::start` (which runs the `--raw` backfill) completes
    // before the attach child is even spawned, so an attached client
    // implies the backfill, and this large capture through `Tmux::run`,
    // already succeeded.
    wait_for_attached(&guard.tmux, &name, &mut child);
    let contents = wait_for_file_containing(&log_path, "oldest-marker");
    assert!(
        contents.len() > 64 * 1024,
        "captured log unexpectedly small: {} bytes",
        contents.len()
    );

    thread::sleep(Duration::from_millis(200));
    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for logged stay");
    assert!(status.success(), "logged attach detach failed: {status}");
    let _ = fs::remove_file(&log_path);
}

#[cfg(unix)]
#[test]
fn default_log_mode_appends_across_attach_detach_cycles_without_duplicating() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    // Trailing filler after the numbered lines so "line-599" itself
    // scrolls off the visible screen into history (default mode's
    // `-E -1` capture never includes whatever is still on-screen, and
    // nothing else pushes the very last produced line off it).
    let command = format!(
        "{}{}sleep 30",
        scroll_filler("line", 600),
        scroll_filler("trailer", 60)
    );
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-log");

    for _ in 0..2 {
        let attach_command = format!(
            "{} attach {} -l {}",
            shell_quote(&executable.to_string_lossy()),
            shell_quote(&name),
            shell_quote(&log_path.to_string_lossy()),
        );
        let mut child = pty_shell_script(&attach_command, &shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("TERM", "xterm-256color")
            .env("PATH", shim.path())
            .env("STAY_TEST_NAMESPACE", &namespace)
            .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
            .spawn()
            .expect("start logged stay");
        wait_for_attached(&guard.tmux, &name, &mut child);
        wait_for_file_containing(&log_path, "line-599");
        // A second attach to an already-logged session finds "line-599"
        // immediately (leftover from the first attach), so the file check
        // alone doesn't prove the relay has entered raw terminal mode yet;
        // see the comment in default_log_mode_produces_a_clean_text_log_with_no_ansi.
        thread::sleep(Duration::from_millis(200));
        child
            .stdin
            .as_mut()
            .expect("stay PTY stdin")
            .write_all(b"\x1c")
            .expect("send stay detach key");
        let status = child.wait().expect("wait for logged stay");
        assert!(status.success(), "logged attach detach failed: {status}");
    }

    let contents = fs::read_to_string(&log_path).expect("read log");
    assert_eq!(contents.matches("line-0\n").count(), 1, "{contents}");
    assert_eq!(contents.matches("line-599\n").count(), 1, "{contents}");
    let _ = fs::remove_file(&log_path);
    let _ = fs::remove_file(offset_sidecar_path(&log_path));
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn default_log_mode_marks_history_eviction_and_keeps_retained_output() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "printf 'old-marker\\n'; i=0; \
                   while [ $i -lt 80 ]; do printf 'initial-%02d\\n' $i; i=$((i+1)); done; \
                   sleep 2; awk 'BEGIN{for(i=0;i<51000;i++) printf \"retained-%05d\\n\", i}'; \
                   sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-eviction-log");
    let attach_command = format!(
        "{} attach {} -l {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&log_path.to_string_lossy()),
    );

    let mut first = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start first eviction attach");
    wait_for_attached(&guard.tmux, &name, &mut first);
    wait_for_file_containing(&log_path, "old-marker");
    thread::sleep(Duration::from_millis(200));
    first
        .stdin
        .as_mut()
        .expect("first eviction attach stdin")
        .write_all(b"\x1c")
        .expect("detach first eviction attach");
    let status = first.wait().expect("wait for first eviction attach");
    assert!(status.success(), "first eviction attach failed: {status}");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = guard
            .tmux
            .run(["capture-pane", "-p", "-t", &name, "-S", "-", "-E", "-"])
            .expect("capture producer output");
        if output
            .stdout
            .windows("retained-50999\n".len())
            .any(|window| window == b"retained-50999\n")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "producer never reached its last line"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let mut second = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start second eviction attach");
    wait_for_attached(&guard.tmux, &name, &mut second);
    let mut contents = String::new();
    for _ in 0..500 {
        contents = fs::read_to_string(&log_path).unwrap_or_default();
        if contents.contains("history evicted before capture") {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        contents.contains("history evicted before capture"),
        "marker missing from log {contents:?}; pane dump: {:?}",
        guard
            .tmux
            .run(["capture-pane", "-p", "-t", &name, "-S", "-", "-E", "-"])
            .expect("capture pane for eviction diagnostic")
            .stdout
    );
    assert!(
        contents.contains("retained-"),
        "retained output missing: {contents}"
    );
    assert!(
        contents.contains("--- history evicted before capture"),
        "marker missing: {contents}"
    );

    thread::sleep(Duration::from_millis(200));
    second
        .stdin
        .as_mut()
        .expect("second eviction attach stdin")
        .write_all(b"\x1c")
        .expect("detach second eviction attach");
    let status = second.wait().expect("wait for second eviction attach");
    assert!(status.success(), "second eviction attach failed: {status}");

    let _ = fs::remove_file(&log_path);
    let _ = fs::remove_file(offset_sidecar_path(&log_path));
}

#[cfg(unix)]
#[test]
fn truncate_log_mode_overwrites_instead_of_appending() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = format!("{}sleep 30", scroll_filler("line", 600));
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-log");

    for _ in 0..2 {
        let attach_command = format!(
            "{} attach {} -l {} -t",
            shell_quote(&executable.to_string_lossy()),
            shell_quote(&name),
            shell_quote(&log_path.to_string_lossy()),
        );
        let mut child = pty_shell_script(&attach_command, &shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("TERM", "xterm-256color")
            .env("PATH", shim.path())
            .env("STAY_TEST_NAMESPACE", &namespace)
            .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
            .spawn()
            .expect("start truncate-logged stay");
        wait_for_attached(&guard.tmux, &name, &mut child);
        wait_for_file_containing(&log_path, "line-599");
        // See the comment in default_log_mode_produces_a_clean_text_log_with_no_ansi.
        thread::sleep(Duration::from_millis(200));
        child
            .stdin
            .as_mut()
            .expect("stay PTY stdin")
            .write_all(b"\x1c")
            .expect("send stay detach key");
        let status = child.wait().expect("wait for truncate-logged stay");
        assert!(status.success(), "truncate-logged detach failed: {status}");
    }

    let contents = fs::read_to_string(&log_path).expect("read log");
    assert_eq!(contents.matches("line-0\n").count(), 1, "{contents}");
    let _ = fs::remove_file(&log_path);
}

#[cfg(unix)]
#[test]
fn raw_log_mode_produces_an_ansi_log_and_keeps_growing_while_detached() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "printf '\\033[31mred marker\\033[0m\\n'; \
                    i=0; while [ $i -lt 100 ]; do sleep 0.05; echo tick-$i; i=$((i+1)); done; \
                    sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-log");
    let attach_command = format!(
        "{} attach {} -l {} --raw",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&log_path.to_string_lossy()),
    );
    let mut child = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start raw-logged stay");

    // Raw mode's backfill write happens synchronously inside
    // `LogSession::start`, before the attach child is even spawned, so the
    // log can contain "red marker" well before the relay puts the terminal
    // in raw mode; see the comment in
    // default_log_mode_produces_a_clean_text_log_with_no_ansi.
    wait_for_attached(&guard.tmux, &name, &mut child);
    thread::sleep(Duration::from_millis(200));
    let contents = wait_for_file_containing(&log_path, "red marker");
    assert!(
        contents.contains('\u{1b}'),
        "expected ANSI bytes: {contents:?}"
    );

    child
        .stdin
        .as_mut()
        .expect("stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = child.wait().expect("wait for raw-logged stay");
    assert!(status.success(), "raw-logged detach failed: {status}");

    let size_at_detach = fs::metadata(&log_path)
        .expect("stat log after detach")
        .len();
    let mut grew = false;
    for _ in 0..500 {
        if fs::metadata(&log_path)
            .expect("stat log while detached")
            .len()
            > size_at_detach
        {
            grew = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(grew, "raw log did not keep growing after detach");
    let _ = fs::remove_file(&log_path);
}

#[cfg(unix)]
#[test]
fn raw_log_mode_reattach_switches_to_the_requested_new_path() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "printf '\\033[31mfirst marker\\033[0m\\n'; \
                    i=0; while [ $i -lt 200 ]; do sleep 0.03; echo tick-$i; i=$((i+1)); done; \
                    sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let first_log = TempPath::file("stay-attachment-first-log");
    let second_log = TempPath::file("stay-attachment-second-log");

    let first_command = format!(
        "{} attach {} -l {} --raw",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&first_log.to_string_lossy()),
    );
    let mut first = pty_shell_script(&first_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start first raw-logged stay");
    wait_for_attached(&guard.tmux, &name, &mut first);
    wait_for_file_containing(&first_log, "first marker");
    first
        .stdin
        .as_mut()
        .expect("first stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send first stay detach key");
    let status = first.wait().expect("wait for first raw-logged stay");
    assert!(status.success(), "first raw-logged detach failed: {status}");

    let second_command = format!(
        "{} attach {} -l {} --raw",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&second_log.to_string_lossy()),
    );
    let mut second = pty_shell_script(&second_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start second raw-logged stay");
    wait_for_attached(&guard.tmux, &name, &mut second);
    let second_contents = wait_for_file_containing(&second_log, "tick-");
    assert!(
        !second_contents.contains("first marker"),
        "new raw target unexpectedly received a backfill: {second_contents:?}"
    );

    second
        .stdin
        .as_mut()
        .expect("second stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send second stay detach key");
    let status = second.wait().expect("wait for second raw-logged stay");
    assert!(
        status.success(),
        "second raw-logged detach failed: {status}"
    );
    let mut stderr = String::new();
    second
        .stdout
        .take()
        .expect("second raw-logged stdout")
        .read_to_string(&mut stderr)
        .expect("read second raw warning");
    assert_eq!(
        stderr.matches("raw logging found an active pipe").count(),
        1,
        "active-pipe warning count: {stderr:?}"
    );
    let _ = fs::remove_file(&first_log);
    let _ = fs::remove_file(&second_log);
}

#[cfg(unix)]
#[test]
fn raw_log_mode_reattach_does_not_truncate_the_still_piping_log() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "printf '\\033[31mred marker\\033[0m\\n'; \
                    i=0; while [ $i -lt 200 ]; do sleep 0.03; echo tick-$i; i=$((i+1)); done; \
                    sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let log_path = TempPath::file("stay-attachment-log");
    let attach_command = format!(
        "{} attach {} -l {} --raw",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&log_path.to_string_lossy()),
    );

    // First attach: backfills "red marker" and starts the pipe.
    let mut first = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start first raw-logged stay");
    wait_for_attached(&guard.tmux, &name, &mut first);
    thread::sleep(Duration::from_millis(200));
    wait_for_file_containing(&log_path, "red marker");
    first
        .stdin
        .as_mut()
        .expect("first stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = first.wait().expect("wait for first raw-logged stay");
    assert!(status.success(), "first raw-logged detach failed: {status}");

    // The pipe keeps running while detached; give it time to accumulate
    // real content before reattaching.
    thread::sleep(Duration::from_secs(2));
    let size_before_reattach = fs::metadata(&log_path)
        .expect("stat log before reattach")
        .len();

    // Second attach against the same still-piping session: must not
    // truncate away what the first attach already logged.
    let mut second = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start second raw-logged stay");
    wait_for_attached(&guard.tmux, &name, &mut second);
    thread::sleep(Duration::from_millis(200));

    let contents_after_reattach = fs::read_to_string(&log_path).expect("read log after reattach");
    assert!(
        contents_after_reattach.contains("red marker"),
        "reattach truncated away the earlier backfill: {} bytes, size before reattach was {size_before_reattach}",
        contents_after_reattach.len(),
    );
    assert!(
        contents_after_reattach.len() as u64 >= size_before_reattach,
        "log shrank across reattach: {} bytes now vs {size_before_reattach} before",
        contents_after_reattach.len(),
    );

    second
        .stdin
        .as_mut()
        .expect("second stay PTY stdin")
        .write_all(b"\x1c")
        .expect("send stay detach key");
    let status = second.wait().expect("wait for second raw-logged stay");
    assert!(
        status.success(),
        "second raw-logged detach failed: {status}"
    );
    let mut stderr = String::new();
    second
        .stdout
        .take()
        .expect("second raw-logged stdout")
        .read_to_string(&mut stderr)
        .expect("read second raw warning");
    assert_eq!(
        stderr.matches("raw logging found an active pipe").count(),
        1,
        "active-pipe warning count: {stderr:?}"
    );
    let _ = fs::remove_file(&log_path);
}

#[cfg(unix)]
#[test]
fn raw_log_mode_warns_for_an_external_active_pipe_and_switches_destination() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "sleep 1; printf 'external first marker\\n'; \
                   i=0; while [ $i -lt 200 ]; do sleep 0.03; echo external-tick-$i; i=$((i+1)); done; \
                   sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let external_log = TempPath::file("stay-attachment-external-log");
    let requested_log = TempPath::file("stay-attachment-requested-log");
    let external_command = format!(
        "umask 077; cat >> {}",
        shell_quote(&external_log.to_string_lossy())
    );
    let _ = guard
        .tmux
        .run(["pipe-pane", "-t", &name, &external_command])
        .expect("start external pipe");
    wait_for_file_containing(&external_log, "external first marker");

    let attach_command = format!(
        "{} attach {} -l {} --raw",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&requested_log.to_string_lossy()),
    );
    let mut child = pty_shell_script(&attach_command, &shim)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start externally-piped raw attach");
    wait_for_attached(&guard.tmux, &name, &mut child);
    let requested_contents = wait_for_file_containing(&requested_log, "external-tick-");
    assert!(
        !requested_contents.contains("external first marker"),
        "external pipe history was backfilled: {requested_contents:?}"
    );

    child
        .stdin
        .as_mut()
        .expect("externally-piped stay stdin")
        .write_all(b"\x1c")
        .expect("detach externally-piped stay");
    let status = child.wait().expect("wait for externally-piped stay");
    assert!(status.success(), "external raw attach failed: {status}");
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .expect("externally-piped stdout")
        .read_to_string(&mut stderr)
        .expect("read external raw warning");
    assert_eq!(
        stderr.matches("raw logging found an active pipe").count(),
        1,
        "active-pipe warning count: {stderr:?}"
    );

    let _ = fs::remove_file(&external_log);
    let _ = fs::remove_file(&requested_log);
}

#[test]
fn production_wrapper_keeps_the_runtime_namespace_fixed_to_stay() {
    let command = Tmux::production().attach_command("work");
    assert_eq!(command.get_args().collect::<Vec<_>>()[1], "stay");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
