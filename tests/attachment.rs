use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
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

struct SessionGuard {
    tmux: Tmux,
}

impl SessionGuard {
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
