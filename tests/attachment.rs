use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
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

fn pty_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn pty_script(executable: &std::path::Path, name: &str) -> Command {
    let mut script = Command::new("script");
    if cfg!(target_os = "linux") {
        let command_string = format!(
            "{} {}",
            shell_quote(&executable.to_string_lossy()),
            shell_quote(name)
        );
        script.args(["-q", "-e", "-c", &command_string, "/dev/null"]);
    } else {
        script.args(["-q", "/dev/null"]);
        script.args([executable.to_str().expect("stay path is UTF-8"), name]);
    }
    script
}

fn pty_shell_script(command: &str) -> Command {
    let mut script = Command::new("script");
    if cfg!(target_os = "linux") {
        script.args(["-q", "-e", "-c", command, "/dev/null"]);
    } else {
        script.args(["-q", "/dev/null", "/bin/sh", "-c", command]);
    }
    script
}

fn wait_for_file_contents(path: &std::path::Path, expected: &str) {
    for _ in 0..200 {
        if let Ok(contents) = fs::read_to_string(path) {
            if contents == expected {
                return;
            }
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
        if let Ok(contents) = fs::read_to_string(path) {
            if !contents.is_empty() {
                return contents;
            }
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
    panic!("timed out waiting for picker output to contain {expected:?}");
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
            "#!/bin/sh\nif [ \"$1\" = \"-L\" ] && [ \"$2\" = \"stay\" ]; then\n    shift 2\n    set -- -L \"$STAY_TEST_NAMESPACE\" \"$@\"\nfi\nif [ -n \"${STAY_TEST_FAIL_LIST_FILE:-}\" ] && [ -f \"$STAY_TEST_FAIL_LIST_FILE\" ] && [ \"$3\" = \"list-panes\" ]; then\n    echo \"picker poll failed\" >&2\n    exit 1\nfi\nexec \"$STAY_TEST_REAL_TMUX\" \"$@\"\n",
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

fn wait_for_pane_exit_status(tmux: &Tmux, name: &str, expected: u8) {
    for _ in 0..250 {
        if tmux.pane_exit_status(name).expect("read pane exit status") == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for pane {name} to exit with {expected}");
}

fn wait_for_terminated_session(tmux: &Tmux, name: &str, expected: u8) {
    for _ in 0..250 {
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
    for _ in 0..250 {
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
    let mut child = pty_script(executable, &name)
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
fn attach_flushes_partial_prompts_before_input_and_after_commands() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let command = "printf 'PROMPT>'; read value; printf 'OUT:%s\\nPROMPT>' \"$value\"; sleep 30";
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {} {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name)
    );
    let mut child = pty_shell_script(&command)
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
        "{} {}; stty -a",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name)
    );
    let mut child = pty_shell_script(&command)
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
fn no_args_on_non_tty_keep_the_plain_inventory_bytes() {
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .output()
        .expect("run non-TTY picker boundary test");
    assert!(output.status.success());
    assert_eq!(output.stdout, format!("{name} [detached]\n").as_bytes());
    drop(guard);
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
    let mut child = pty_shell_script(&command)
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
    let mut child = pty_shell_script(&command)
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
    wait_for_output_contains(&observed_output, "Esc");
    wait_for_output_contains(&observed_output, "quit");

    let stdin = child.stdin.as_mut().expect("empty picker stdin");
    stdin.write_all(b"\r").expect("press Enter in empty picker");
    wait_for_output_contains(&observed_output, "New session name:");
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
fn picker_quit_restores_the_outer_terminal() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!("{}; stty -a", shell_quote(&executable.to_string_lossy()));
    let mut child = pty_shell_script(&command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker terminal test");

    thread::sleep(Duration::from_millis(500));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit picker");
    let result = child
        .wait_with_output()
        .expect("wait for picker terminal test");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.status.success(), "picker quit failed: {output}");
    assert!(output.contains("icanon"), "terminal remained raw: {output}");
    assert!(
        output.contains("echo"),
        "terminal echo was not restored: {output}"
    );
    drop(guard);
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
    let mut child = pty_shell_script(&command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker create test");

    thread::sleep(Duration::from_millis(500));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(format!("c{name}\r").as_bytes())
        .expect("create picker session");
    wait_for_attached(&guard.tmux, &name, &mut child);
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1c")
        .expect("detach created picker session");
    assert!(child.wait().expect("wait for picker create test").success());
    assert!(guard
        .tmux
        .list_sessions()
        .expect("list created picker session")
        .iter()
        .any(|session| session.name == name));
    drop(guard);
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
    let mut child = pty_shell_script(&command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker kill test");

    thread::sleep(Duration::from_millis(500));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"\x1b[Bk\r")
        .expect("confirm picker kill with default no");
    thread::sleep(Duration::from_millis(500));
    assert!(guard
        .tmux
        .list_sessions()
        .expect("list after default-no confirmation")
        .iter()
        .any(|session| session.name == name));

    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"k\x1b")
        .expect("cancel picker kill");
    thread::sleep(Duration::from_millis(500));
    assert!(guard
        .tmux
        .list_sessions()
        .expect("list after cancelled confirmation")
        .iter()
        .any(|session| session.name == name));

    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"k\x1b[D\r")
        .expect("confirm picker kill with yes");
    thread::sleep(Duration::from_millis(700));
    child
        .stdin
        .as_mut()
        .expect("picker stdin")
        .write_all(b"q")
        .expect("quit after picker kill");
    assert!(child.wait().expect("wait for picker kill test").success());
    assert!(guard
        .tmux
        .list_sessions()
        .expect("list after picker kill")
        .is_empty());
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_forwards_typed_ahead_input_to_the_attached_session() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = std::env::temp_dir().join(unique_name());
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
    let mut child = pty_shell_script(&command)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("PATH", shim.path())
        .env("STAY_TEST_NAMESPACE", &namespace)
        .env("STAY_TEST_REAL_TMUX", &shim.real_tmux)
        .spawn()
        .expect("start picker handoff test");

    thread::sleep(Duration::from_millis(500));
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
    assert!(child.wait().expect("wait for picker handoff").success());
    let _ = fs::remove_file(marker);
    let _ = fs::remove_dir(root);
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
    let mut child = pty_shell_script(&command)
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
    assert!(child
        .wait()
        .expect("wait for picker identity test")
        .success());
    drop(guard);
}

#[cfg(unix)]
#[test]
fn picker_retains_its_last_list_when_a_poll_fails() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = std::env::temp_dir().join(unique_name());
    fs::create_dir(&root).expect("create picker poll directory");
    let failure_marker = root.join("fail-list");
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "stty rows 24 cols 80; exec {}",
        shell_quote(&executable.to_string_lossy())
    );
    let mut child = pty_shell_script(&command)
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
        rendered_output.contains("picker")
            && rendered_output.contains("poll")
            && rendered_output.contains("failed"),
        "poll error was not rendered: {output}"
    );
    let _ = fs::remove_file(failure_marker);
    let _ = fs::remove_dir(root);
    drop(guard);
}

#[cfg(unix)]
#[test]
fn sigterm_detaches_and_restores_cooked_terminal_settings() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let root = std::env::temp_dir().join(unique_name());
    fs::create_dir(&root).expect("create SIGTERM test directory");
    let pid_path = root.join("stay.pid");
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let command = format!(
        "{} {} & echo $! > {}; wait $!; stty -a",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&name),
        shell_quote(&pid_path.to_string_lossy())
    );
    let mut child = pty_shell_script(&command)
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
    let _ = fs::remove_dir(root);
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
    let mut child = pty_script(executable, &name)
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
    let root = std::env::temp_dir().join(unique_name());
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
    let mut child = pty_script(executable, &name)
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
    let _ = fs::remove_dir(root);
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
    let mut child = pty_script(executable, &name)
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
    let mut child = pty_script(executable, &name)
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
    let mut child = pty_script(executable, &name)
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
    assert!(guard
        .tmux
        .list_sessions()
        .expect("list retained terminated session")
        .iter()
        .any(|session| session.name == name && !session.attached));
    assert_eq!(guard.tmux.pane_exit_status(&name).unwrap(), Some(7));
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
    let mut child = pty_script(executable, &name)
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
    let root = std::env::temp_dir().join(unique_name());
    fs::create_dir(&root).expect("create manual-detach test directory");
    let marker = root.join("ended");
    let command = format!(
        "sleep 1; printf done > {}; exit 9",
        shell_quote(&marker.to_string_lossy())
    );
    let guard = SessionGuard::new_with_command(namespace.clone(), &name, &["sh", "-c", &command]);
    let shim = TmuxShim::new();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_stay"));
    let mut child = pty_script(executable, &name)
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
    let _ = fs::remove_dir(root);
}

#[cfg(unix)]
#[test]
fn redirected_stdin_still_uses_the_attach_pty() {
    let _lock = pty_test_lock();
    let name = unique_name();
    let namespace = unique_namespace();
    let guard = SessionGuard::new(namespace.clone(), &name);
    let shim = TmuxShim::new();
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg(&name)
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
