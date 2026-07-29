use std::fs;
use std::process::Stdio;
#[cfg(target_os = "linux")]
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use stay::{config::Config, session, tmux::Tmux};

fn unique_namespace() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("stay-test-{pid}-{nanos}-{counter}")
}

fn unique_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

struct ServerGuard {
    tmux: Tmux,
    #[cfg(target_os = "linux")]
    namespace: String,
}

impl ServerGuard {
    #[cfg(target_os = "linux")]
    fn new() -> Self {
        let namespace = unique_namespace();
        Self {
            tmux: Tmux::for_test_namespace(namespace.clone()),
            namespace,
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn new() -> Self {
        Self {
            tmux: Tmux::for_test_namespace(unique_namespace()),
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

fn create_session(
    guard: &ServerGuard,
    config: &Config,
    session_name: &str,
    cwd: Option<&std::path::Path>,
    command: &[String],
) {
    session::create_session_with_shell(
        &guard.tmux,
        config,
        session_name,
        cwd.map(|path| path.to_str().unwrap()),
        command,
        std::path::Path::new("/bin/sh"),
        None,
    )
    .unwrap();
}

fn stdout_string(tmux: &Tmux, arguments: &[&str]) -> String {
    let output = tmux.run(arguments.iter().copied()).unwrap();
    assert!(output.status.success(), "tmux command failed");
    String::from_utf8(output.stdout).unwrap()
}

#[cfg(target_os = "linux")]
fn start_tmux_client(namespace: &str, session_name: &str, flags: Option<&str>) -> Child {
    let mut script = Command::new("script");
    let attach_flags = flags.map_or_else(String::new, |flags| format!(" -f {flags}"));
    let command = format!("tmux -L {namespace} attach-session{attach_flags} -t {session_name}");
    script.args(["-q", "-e", "-c", &command, "/dev/null"]);
    script
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .spawn()
        .expect("start tmux client")
}

#[cfg(target_os = "linux")]
fn wait_for_client(tmux: &Tmux, child: &mut Child) -> String {
    for _ in 0..200 {
        let clients = stdout_string(tmux, &["list-clients", "-F", "#{client_name}"]);
        if let Some(client_name) = clients.lines().next() {
            return client_name.to_owned();
        }
        if let Some(status) = child.try_wait().expect("check tmux client status") {
            panic!("tmux client exited before attaching: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for tmux client");
}

fn wait_for_file(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_for_session(tmux: &Tmux, session_name: &str) {
    for _ in 0..100 {
        let sessions = stdout_string(tmux, &["list-sessions", "-F", "#{session_name}"]);
        if sessions.lines().any(|line| line == session_name) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for session {session_name}");
}

fn wait_for_live_pane(tmux: &Tmux, session_name: &str) {
    for _ in 0..100 {
        let panes = stdout_string(
            tmux,
            &["list-panes", "-a", "-F", "#{session_name}:#{pane_dead}"],
        );
        if panes
            .lines()
            .any(|line| line == format!("{session_name}:0"))
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for live pane {session_name}");
}

fn wait_for_dead_pane(tmux: &Tmux, session_name: &str, status: &str) {
    // CI can schedule the tmux server and its pane command for longer than
    // the command's nominal one-second runtime. Keep polling long enough to
    // observe remain-on-exit without weakening the expected pane status.
    for _ in 0..250 {
        let panes = stdout_string(
            tmux,
            &[
                "list-panes",
                "-a",
                "-F",
                "#{session_name}:#{pane_dead}:#{pane_dead_status}",
            ],
        );
        if panes
            .lines()
            .any(|line| line == format!("{session_name}:1:{status}"))
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for dead pane {session_name}");
}

#[test]
fn creates_session_with_cwd_environment_history_limit_and_remain_on_exit() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 4321,
        log_capture_interval_seconds: 5,
    };
    let root = unique_path("stay-create");
    fs::create_dir_all(&root).unwrap();
    let cwd = root.join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    let pwd_file = root.join("pwd.txt");
    let env_file = root.join("env.txt");
    let command = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        format!(
            "pwd > {}; printf '%s' \"$STAY_SESSION_NAME\" > {}; sleep 1",
            pwd_file.display(),
            env_file.display()
        ),
    ];

    create_session(&guard, &config, "create", Some(&cwd), &command);
    wait_for_file(&pwd_file);
    wait_for_file(&env_file);

    assert_eq!(
        fs::read_to_string(&pwd_file).unwrap().trim(),
        cwd.to_string_lossy()
    );
    assert_eq!(fs::read_to_string(&env_file).unwrap(), "create");

    let history_limit = stdout_string(
        &guard.tmux,
        &[
            "display-message",
            "-p",
            "-t",
            "create:0",
            "#{history_limit}",
        ],
    );
    assert_eq!(history_limit.trim(), "4321");

    wait_for_dead_pane(&guard.tmux, "create", "0");
    let sessions = stdout_string(&guard.tmux, &["list-sessions", "-F", "#{session_name}"]);
    assert!(sessions.lines().any(|line| line == "create"));
}

#[cfg(target_os = "linux")]
#[test]
fn built_in_status_shows_each_client_attachment_modifier() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 1000,
        log_capture_interval_seconds: 5,
    };
    create_session(
        &guard,
        &config,
        "status",
        None,
        &["sleep".to_owned(), "30".to_owned()],
    );

    let cases = [
        (None, ""),
        (Some("read-only"), "(view only)"),
        (Some("ignore-size"), "(low priority)"),
        (Some("read-only,ignore-size"), "(view only / low priority)"),
    ];
    for (flags, expected_label) in cases {
        let mut child = start_tmux_client(&guard.namespace, "status", flags);
        let client_name = wait_for_client(&guard.tmux, &mut child);
        let rendered = stdout_string(
            &guard.tmux,
            &[
                "display-message",
                "-p",
                "-t",
                client_name.as_str(),
                "#{E:status-left}",
            ],
        );
        assert!(rendered.contains("status"));
        assert_eq!(
            rendered.matches("(view only)").count(),
            usize::from(flags == Some("read-only"))
        );
        assert_eq!(
            rendered.matches("(low priority)").count(),
            usize::from(flags == Some("ignore-size"))
        );
        assert_eq!(
            rendered.matches("(view only / low priority)").count(),
            usize::from(flags == Some("read-only,ignore-size"))
        );
        assert!(
            expected_label.is_empty() || rendered.contains(expected_label),
            "status for {flags:?} did not contain {expected_label:?}: {rendered:?}"
        );

        let status = guard
            .tmux
            .command(["detach-client", "-t", client_name.as_str()])
            .status()
            .expect("detach tmux client");
        assert!(status.success());
        child.wait().expect("wait for detached tmux client");
    }
}

#[test]
fn passes_explicit_arguments_verbatim_even_with_shell_metacharacters() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 9000,
        log_capture_interval_seconds: 5,
    };
    let root = unique_path("stay-argv");
    fs::create_dir_all(&root).unwrap();
    let script = root.join("capture-argv.sh");
    let args_file = root.join("args.txt");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" > {}\nsleep 1\n",
            args_file.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }

    let command = vec![
        script.to_string_lossy().into_owned(),
        "alpha beta".to_owned(),
        "semi; $(touch /tmp/stay-ignored)".to_owned(),
    ];

    create_session(&guard, &config, "argv", None, &command);
    wait_for_file(&args_file);

    assert_eq!(
        fs::read_to_string(&args_file).unwrap(),
        "alpha beta\nsemi; $(touch /tmp/stay-ignored)\n"
    );
}

#[test]
fn default_command_uses_the_configured_shell_and_preserves_quoting() {
    let guard = ServerGuard::new();
    let root = unique_path("stay-shell");
    fs::create_dir_all(&root).unwrap();
    let output_file = root.join("output.txt");
    let config = Config {
        default_command: Some(format!(
            "printf '%s' 'quoted value; preserved' > {}",
            output_file.display()
        )),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 3000,
        log_capture_interval_seconds: 5,
    };
    session::create_session_with_shell(
        &guard.tmux,
        &config,
        "shell",
        None,
        &[],
        std::path::Path::new("/bin/sh"),
        None,
    )
    .unwrap();
    wait_for_file(&output_file);

    assert_eq!(
        fs::read_to_string(&output_file).unwrap(),
        "quoted value; preserved"
    );
}

#[test]
fn no_default_command_runs_one_interactive_shell_with_shell_set_and_unset() {
    let root = unique_path("stay-shell-direct");
    fs::create_dir_all(&root).unwrap();
    let wrapper = root.join("shell-wrapper.sh");
    let wrapper_tmp = root.join("shell-wrapper.tmp");
    let argv_file = root.join("argv.txt");
    fs::write(
        &wrapper_tmp,
        format!(
            "#!/bin/sh\nprintf '%s\\n%s\\n%s\\n' \"$#\" \"$1\" \"$2\" > {}\n",
            argv_file.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&wrapper_tmp).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper_tmp, permissions).unwrap();
    }
    fs::rename(&wrapper_tmp, &wrapper).unwrap();

    let config = Config {
        default_command: None,
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 1000,
        log_capture_interval_seconds: 5,
    };
    {
        let guard = ServerGuard::new();
        session::create_session_with_shell(
            &guard.tmux,
            &config,
            "shell-set",
            None,
            &[],
            &wrapper,
            None,
        )
        .unwrap();
        wait_for_file(&argv_file);
        assert_eq!(fs::read_to_string(&argv_file).unwrap(), "0\n\n\n");
    }

    let guard = ServerGuard::new();
    session::create_session_with_shell(
        &guard.tmux,
        &config,
        "shell-unset",
        None,
        &[],
        std::path::Path::new("/bin/sh"),
        None,
    )
    .unwrap();
    wait_for_live_pane(&guard.tmux, "shell-unset");
}

#[test]
fn quick_exits_are_retained_and_report_their_statuses() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 2000,
        log_capture_interval_seconds: 5,
    };

    create_session(
        &guard,
        &config,
        "exit-1",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "exit 1".to_owned()],
    );
    create_session(
        &guard,
        &config,
        "exit-127",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "exit 127".to_owned()],
    );

    wait_for_dead_pane(&guard.tmux, "exit-1", "1");
    wait_for_dead_pane(&guard.tmux, "exit-127", "127");
    wait_for_session(&guard.tmux, "exit-1");
    wait_for_session(&guard.tmux, "exit-127");

    let sessions = stdout_string(&guard.tmux, &["list-sessions", "-F", "#{session_name}"]);
    assert!(sessions.lines().any(|line| line == "exit-1"));
    assert!(sessions.lines().any(|line| line == "exit-127"));
}

#[test]
fn rejects_missing_or_non_executable_explicit_commands_before_tmux_creation() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 1000,
        log_capture_interval_seconds: 5,
    };
    let script = unique_path("stay-not-exec");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();

    let error = session::create_session_with_shell(
        &guard.tmux,
        &config,
        "missing",
        None,
        &[script.to_string_lossy().into_owned()],
        std::path::Path::new("/bin/sh"),
        None,
    )
    .unwrap_err();
    assert!(error.contains("not a regular executable") || error.contains("cannot be executed"));

    assert!(guard.tmux.list_sessions().unwrap().is_empty());
}

#[test]
fn kill_session_removes_an_existing_session_without_replacing_it() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 1000,
        log_capture_interval_seconds: 5,
    };

    create_session(
        &guard,
        &config,
        "kill-me",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "sleep 10".to_owned()],
    );

    wait_for_session(&guard.tmux, "kill-me");
    session::kill_session(&guard.tmux, "kill-me").unwrap();
    assert!(guard.tmux.list_sessions().unwrap().is_empty());
}

#[test]
fn kill_session_reports_missing_sessions_clearly() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 1000,
        log_capture_interval_seconds: 5,
    };

    create_session(
        &guard,
        &config,
        "other",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "sleep 10".to_owned()],
    );

    let error = session::kill_session(&guard.tmux, "missing").unwrap_err();
    assert!(error.contains("can't find session") || error.contains("no such session"));
}

#[test]
fn force_recreate_replaces_a_live_session_with_a_new_command() {
    let guard = ServerGuard::new();
    let root = unique_path("stay-force-live");
    fs::create_dir_all(&root).unwrap();
    let marker = root.join("marker.txt");
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 2000,
        log_capture_interval_seconds: 5,
    };

    create_session(
        &guard,
        &config,
        "swap",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "sleep 10".to_owned()],
    );

    session::force_recreate_session(
        &guard.tmux,
        &config,
        "swap",
        None,
        &[
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!("printf new > {}; sleep 1", marker.display()),
        ],
    )
    .unwrap();

    wait_for_file(&marker);
    let sessions = stdout_string(&guard.tmux, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(sessions.lines().collect::<Vec<_>>(), ["swap"]);
}

#[test]
fn force_recreate_creates_a_session_when_the_server_has_not_started() {
    let guard = ServerGuard::new();
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 2000,
        log_capture_interval_seconds: 5,
    };

    session::force_recreate_session(
        &guard.tmux,
        &config,
        "fresh",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "sleep 1".to_owned()],
    )
    .unwrap();

    wait_for_session(&guard.tmux, "fresh");
    let sessions = stdout_string(&guard.tmux, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(sessions.lines().collect::<Vec<_>>(), ["fresh"]);
}

#[test]
fn force_recreate_replaces_an_already_dead_session_with_a_new_command() {
    let guard = ServerGuard::new();
    let root = unique_path("stay-force-dead");
    fs::create_dir_all(&root).unwrap();
    let marker = root.join("marker.txt");
    let config = Config {
        default_command: Some("ignored".to_owned()),
        detach_key: 0x1c,
        copy_mode_key: 0,
        history_lines: 2000,
        log_capture_interval_seconds: 5,
    };

    create_session(
        &guard,
        &config,
        "swap",
        None,
        &["/bin/sh".to_owned(), "-c".to_owned(), "exit 1".to_owned()],
    );
    wait_for_dead_pane(&guard.tmux, "swap", "1");

    session::force_recreate_session(
        &guard.tmux,
        &config,
        "swap",
        None,
        &[
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!("printf recreated > {}; sleep 1", marker.display()),
        ],
    )
    .unwrap();

    wait_for_file(&marker);
    let sessions = stdout_string(&guard.tmux, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(sessions.lines().collect::<Vec<_>>(), ["swap"]);
}
