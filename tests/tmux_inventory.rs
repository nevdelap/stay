use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use stay::tmux::{SessionRecord, Tmux, render_session_inventory, test_tmux_tmpdir};

mod support;
use support::TempPath;

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

struct ServerGuard {
    namespace: String,
    tmux: Tmux,
}

impl ServerGuard {
    fn new() -> Self {
        let namespace = unique_namespace();
        Self {
            tmux: Tmux::for_test_namespace(namespace.clone()),
            namespace,
        }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = run_tmux_status(&self.tmux, ["kill-server"], "kill inventory test server");
    }
}

fn create_sleeping_session(tmux: &Tmux, name: &str) {
    run_tmux_status(
        tmux,
        ["new-session", "-d", "-s", name, "--", "sleep", "10"],
        "start test session",
    )
    .expect("start test session");
}

fn run_tmux_status<I, S>(tmux: &Tmux, arguments: I, description: &str) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = tmux
        .run(arguments)
        .map_err(|error| format!("{description}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{description}: {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn create_terminating_session(
    tmux: &Tmux,
    name: &str,
    ready: &std::path::Path,
    release: &std::path::Path,
    completion: &std::path::Path,
) {
    run_tmux_status(
        tmux,
        ["new-session", "-d", "-s", name, "--", "sleep", "30"],
        "start terminating test session",
    )
    .expect("start terminating test session");
    run_tmux_status(
        tmux,
        ["set-window-option", "-t", name, "remain-on-exit", "on"],
        "retain terminating test session",
    )
    .expect("retain terminating test session");
    let command = format!(
        ": > {}; while test ! -e {}; do sleep 0.01; done; : > {}; sleep 1; exit 7",
        ready.display(),
        release.display(),
        completion.display()
    );
    let mut arguments = vec![
        "respawn-pane",
        "-k",
        "-t",
        name,
        "sh",
        "-c",
        command.as_str(),
    ];
    run_tmux_status(tmux, arguments.drain(..), "start terminating test command")
        .expect("start terminating test command");
}

fn wait_for_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if path.exists() {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_for_exit_status(tmux: &Tmux, name: &str, expected: u8) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if tmux
            .pane_exit_status(name)
            .expect("read terminating test status")
            == Some(expected)
        {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let panes = tmux
        .run([
            "list-panes",
            "-t",
            name,
            "-F",
            "#{pane_dead}:#{pane_dead_status}:#{pane_dead_signal}:#{pane_dead_time}:#{pane_pid}:#{pane_current_command}",
        ])
        .expect("read final terminating pane status");
    let all_panes = tmux
        .run([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{pane_dead}:#{pane_dead_status}:#{pane_dead_signal}:#{pane_dead_time}:#{pane_pid}:#{pane_current_command}:#{pane_start_command}",
        ])
        .expect("read final inventory pane statuses");
    let remain_on_exit = tmux
        .run(["show-window-options", "-t", name, "-v", "remain-on-exit"])
        .expect("read final remain-on-exit setting");
    panic!(
        "timed out waiting for {name} to exit with {expected}; socket root: {}; target panes: {:?}; all panes: {:?}; remain-on-exit: {:?}",
        test_tmux_tmpdir().display(),
        String::from_utf8_lossy(&panes.stdout),
        String::from_utf8_lossy(&all_panes.stdout),
        String::from_utf8_lossy(&remain_on_exit.stdout),
    );
}

fn wait_for_any_pane_exit_status(tmux: &Tmux, name: &str, expected: u8) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let output = tmux
            .run([
                "list-panes",
                "-t",
                name,
                "-F",
                "#{pane_dead}:#{pane_dead_status}",
            ])
            .expect("read mixed session pane statuses");
        if String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|row| row.trim() == format!("1:{expected}"))
        {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let panes = tmux
        .run([
            "list-panes",
            "-t",
            name,
            "-F",
            "#{pane_dead}:#{pane_dead_status}:#{pane_dead_signal}:#{pane_dead_time}:#{pane_pid}:#{pane_current_command}",
        ])
        .expect("read final mixed pane statuses");
    panic!(
        "timed out waiting for a pane in {name} to exit with {expected}; final panes: {:?}",
        String::from_utf8_lossy(&panes.stdout)
    );
}

fn wait_for_attached_session(tmux: &Tmux, session_name: &str, attached_client: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let sessions = tmux.list_sessions().expect("list attached session");
        if sessions
            .iter()
            .any(|session| session.name == session_name && session.attached)
        {
            return;
        }
        if let Some(status) = attached_client
            .try_wait()
            .expect("check attached tmux client")
        {
            panic!("tmux attach client exited before attaching: {status}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for attached session {session_name}");
}

#[test]
fn real_tmux_inventory_orders_names_and_creation_times() {
    let guard = ServerGuard::new();
    create_sleeping_session(&guard.tmux, "alpha");
    thread::sleep(Duration::from_secs(1));
    create_sleeping_session(&guard.tmux, "zeta");

    let sessions = guard.tmux.list_sessions().expect("list test sessions");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name, "alpha");
    assert_eq!(sessions[1].name, "zeta");
    assert!(sessions[0].created < sessions[1].created);
    assert!(
        sessions
            .iter()
            .all(|session| session.status_word() == "detached")
    );
    assert!(sessions.iter().all(|session| !session.terminated));
    assert!(sessions.iter().all(|session| session.exit_code.is_none()));
    assert!(sessions.iter().all(|session| session.dead_time.is_none()));
}

#[test]
fn real_tmux_inventory_reports_a_terminated_session() {
    let guard = ServerGuard::new();
    run_tmux_status(
        &guard.tmux,
        ["new-session", "-d", "-s", "live", "--", "sleep", "60"],
        "start long-lived test session",
    )
    .expect("start long-lived test session");
    let release = TempPath::file("stay-inventory-release");
    let ready = TempPath::file("stay-inventory-ready");
    let completion = TempPath::file("stay-inventory-completion");
    create_terminating_session(&guard.tmux, "dead", &ready, &release, &completion);
    wait_for_file(&ready);
    fs::write(&release, "").expect("release terminating inventory session");
    wait_for_file(&completion);
    wait_for_exit_status(&guard.tmux, "dead", 7);

    let sessions = guard.tmux.list_sessions().expect("list test sessions");
    let live = sessions
        .iter()
        .find(|session| session.name == "live")
        .unwrap();
    assert!(!live.terminated);
    assert_eq!(live.exit_code, None);
    assert_eq!(live.dead_time, None);

    let dead = sessions
        .iter()
        .find(|session| session.name == "dead")
        .unwrap();
    assert_eq!(dead.status_word(), "terminated");
    assert!(dead.terminated);
    assert_eq!(dead.exit_code, Some(7));
    assert!(dead.dead_time.is_some());
}

#[test]
fn real_tmux_inventory_keeps_mixed_live_and_dead_sessions_alive() {
    let guard = ServerGuard::new();
    let release = TempPath::file("stay-inventory-release");
    let ready = TempPath::file("stay-inventory-ready");
    let completion = TempPath::file("stay-inventory-completion");
    create_terminating_session(&guard.tmux, "mixed", &ready, &release, &completion);
    wait_for_file(&ready);
    run_tmux_status(
        &guard.tmux,
        ["split-window", "-t", "mixed:0", "-h", "--", "sleep", "30"],
        "split mixed test session",
    )
    .expect("split mixed test session");
    fs::write(&release, "").expect("release mixed inventory session");
    wait_for_file(&completion);
    wait_for_any_pane_exit_status(&guard.tmux, "mixed", 7);

    let session = guard
        .tmux
        .list_sessions()
        .expect("list mixed test session")
        .into_iter()
        .find(|session| session.name == "mixed")
        .expect("mixed session row");
    assert_eq!(session.status_word(), "detached");
    assert!(!session.terminated);
    assert_eq!(session.exit_code, None);
    assert_eq!(session.dead_time, None);
}

#[test]
fn real_tmux_inventory_preserves_colons_in_dynamic_fields() {
    let guard = ServerGuard::new();
    let root = TempPath::directory("stay-inventory:");
    let expected_root = fs::canonicalize(&root).expect("canonicalize working directory");
    let command = root.join("cmd:colon");
    fs::copy("/bin/sh", &command).expect("copy colon-containing shell");
    let mut permissions = fs::metadata(&command).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&command, permissions).unwrap();

    let arguments = vec![
        OsString::from("new-session"),
        OsString::from("-d"),
        OsString::from("-s"),
        OsString::from("dynamic"),
        OsString::from("-c"),
        root.as_os_str().to_owned(),
        OsString::from("--"),
        command.as_os_str().to_owned(),
        OsString::from("-c"),
        OsString::from("sleep 10"),
    ];
    run_tmux_status(&guard.tmux, arguments, "start dynamic-field session")
        .expect("start dynamic-field session");

    let deadline = Instant::now() + Duration::from_secs(10);
    let session = loop {
        let session = guard
            .tmux
            .list_sessions()
            .expect("list dynamic-field session")
            .into_iter()
            .find(|session| session.name == "dynamic");
        if let Some(session) = session.as_ref()
            && session.current_directory.is_some()
            && session.current_command.is_some()
        {
            break session.clone();
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for dynamic-field session: {session:?}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(session.current_directory.as_deref(), expected_root.to_str());
    // tmux reports the running process name, which differs by platform for a
    // renamed shell and its child. Presence is the portable contract here;
    // exact colon decoding is covered by the parser test.
    assert!(session.current_command.is_some());
}

#[test]
fn real_tmux_inventory_round_trips_control_characters_in_dynamic_fields() {
    let guard = ServerGuard::new();
    let root = TempPath::directory("stay-inventory-controls");
    let cwd = root.join("cwd\nreturn\runit\u{1f}\\slash");
    fs::create_dir_all(&cwd).expect("create control-character cwd");
    let expected_cwd = fs::canonicalize(&cwd).expect("canonicalize control-character cwd");
    let command = cwd.join("cmd\nreturn\runit\u{1f}end");
    fs::copy("/bin/sh", &command).expect("copy control-character shell");
    let mut permissions = fs::metadata(&command).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&command, permissions).unwrap();

    run_tmux_status(
        &guard.tmux,
        [
            OsString::from("new-session"),
            OsString::from("-d"),
            OsString::from("-s"),
            OsString::from("controls"),
            OsString::from("-c"),
            cwd.as_os_str().to_owned(),
            OsString::from("--"),
            command.as_os_str().to_owned(),
            OsString::from("-c"),
            OsString::from("sleep 60"),
        ],
        "start control-character session",
    )
    .expect("start control-character session");

    let deadline = Instant::now() + Duration::from_secs(10);
    let session = loop {
        let session = guard
            .tmux
            .list_sessions()
            .expect("list control-character session")
            .into_iter()
            .find(|session| session.name == "controls");
        if let Some(session) = session.as_ref().filter(|session| {
            session.current_directory.as_deref() == expected_cwd.to_str()
                && session.current_command.is_some()
        }) {
            break session.clone();
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for control-character fields: {session:?}"
        );
        thread::sleep(Duration::from_millis(20));
    };

    let rendered = render_session_inventory(&[session], false);
    assert!(rendered.starts_with("controls"));
    assert!(rendered.ends_with("[detached]\n"));
}

#[test]
fn real_tmux_can_rename_a_session() {
    let guard = ServerGuard::new();
    create_sleeping_session(&guard.tmux, "before");

    guard
        .tmux
        .rename_session("before", "after")
        .expect("rename test session");
    let sessions = guard.tmux.list_sessions().expect("list renamed session");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "after");
}

#[test]
fn real_tmux_missing_server_is_empty() {
    let guard = ServerGuard::new();
    assert!(
        guard
            .tmux
            .list_sessions()
            .expect("empty inventory")
            .is_empty()
    );
}

#[test]
fn real_tmux_inventory_marks_attached_clients() {
    let guard = ServerGuard::new();
    create_sleeping_session(&guard.tmux, "alpha");

    let mut attached_command = std::process::Command::new("script");
    if cfg!(target_os = "linux") {
        let command = format!("tmux -L {} attach-session -t alpha", guard.namespace);
        attached_command.args(["-q", "-e", "-c", &command, "/dev/null"]);
    } else {
        attached_command.args(["-q", "/dev/null"]);
        attached_command.args([
            "tmux",
            "-L",
            &guard.namespace,
            "attach-session",
            "-t",
            "alpha",
        ]);
    }
    let mut attached_client = attached_command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .env("TMUX_TMPDIR", test_tmux_tmpdir())
        .spawn()
        .expect("start attached tmux client");

    wait_for_attached_session(&guard.tmux, "alpha", &mut attached_client);
    let sessions = guard.tmux.list_sessions().expect("list attached session");
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].attached);
    assert_eq!(sessions[0].status_word(), "attached");

    let _ = attached_client.kill();
    let _ = attached_client.wait();
}

#[test]
fn render_session_inventory_uses_exact_tab_separated_bytes() {
    let sessions = vec![
        SessionRecord {
            name: "alpha".to_owned(),
            attached: false,
            created: 1,
            terminated: false,
            exit_code: None,
            dead_signal: None,
            dead_time: None,
            current_directory: None,
            current_command: None,
        },
        SessionRecord {
            name: "work 東京".to_owned(),
            attached: true,
            created: 2,
            terminated: false,
            exit_code: None,
            dead_signal: None,
            dead_time: None,
            current_directory: None,
            current_command: None,
        },
    ];

    assert_eq!(
        render_session_inventory(&sessions, false),
        "alpha     [detached]\nwork 東京 [attached]\n"
    );
}

#[test]
fn session_status_details_render_exit_time_and_conditional_red() {
    let sessions = vec![SessionRecord {
        name: "job".to_owned(),
        attached: false,
        created: 1,
        terminated: true,
        exit_code: Some(7),
        dead_signal: None,
        dead_time: Some(0),
        current_directory: None,
        current_command: None,
    }];

    let plain = render_session_inventory(&sessions, false);
    assert!(plain.starts_with("job [terminated exit=7 @"));
    assert!(plain.ends_with("]\n"));
    assert!(!plain.contains("\x1b[31m"));

    let coloured = render_session_inventory(&sessions, true);
    assert!(coloured.contains("\x1b[31m7\x1b[0m"));
}

#[test]
fn session_status_details_render_a_signal_killed_pane() {
    let sessions = vec![SessionRecord {
        name: "job".to_owned(),
        attached: false,
        created: 1,
        terminated: true,
        exit_code: None,
        dead_signal: Some(9),
        dead_time: Some(0),
        current_directory: None,
        current_command: None,
    }];

    let plain = render_session_inventory(&sessions, false);
    assert!(plain.starts_with("job [terminated signal=9 @"));
    assert!(plain.ends_with("]\n"));
    assert!(!plain.contains("\x1b[31m"));

    let coloured = render_session_inventory(&sessions, true);
    assert!(coloured.contains("\x1b[31m9\x1b[0m"));
}

#[test]
fn render_session_inventory_uses_zero_bytes_for_empty_lists() {
    let sessions: Vec<SessionRecord> = Vec::new();
    assert_eq!(render_session_inventory(&sessions, false), "");
}
