use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use stay::tmux::{render_session_inventory, SessionRecord, Tmux};

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
        let _ = self
            .tmux
            .command(["kill-server"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn create_sleeping_session(tmux: &Tmux, name: &str) {
    let status = tmux
        .command(["new-session", "-d", "-s", name, "--", "sleep", "10"])
        .status()
        .expect("start test session");
    assert!(status.success(), "tmux failed to create {name}");
}

fn wait_for_attached_session(tmux: &Tmux, session_name: &str) {
    for _ in 0..100 {
        let sessions = tmux.list_sessions().expect("list attached session");
        if sessions
            .iter()
            .any(|session| session.name == session_name && session.attached)
        {
            return;
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
    assert!(sessions.iter().all(|session| session.marker() == 'd'));
}

#[test]
fn real_tmux_missing_server_is_empty() {
    let guard = ServerGuard::new();
    assert!(guard
        .tmux
        .list_sessions()
        .expect("empty inventory")
        .is_empty());
}

#[test]
fn real_tmux_inventory_marks_attached_clients() {
    let guard = ServerGuard::new();
    create_sleeping_session(&guard.tmux, "alpha");

    let mut attached_command = std::process::Command::new("script");
    attached_command.args(["-q", "/dev/null"]);
    if cfg!(target_os = "linux") {
        attached_command.arg("--");
    }
    let mut attached_client = attached_command
        .args([
            "tmux",
            "-L",
            &guard.namespace,
            "attach-session",
            "-t",
            "alpha",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TERM", "xterm-256color")
        .spawn()
        .expect("start attached tmux client");

    wait_for_attached_session(&guard.tmux, "alpha");
    let sessions = guard.tmux.list_sessions().expect("list attached session");
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].attached);
    assert_eq!(sessions[0].marker(), 'a');

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
        },
        SessionRecord {
            name: "work 東京".to_owned(),
            attached: true,
            created: 2,
        },
    ];

    assert_eq!(
        render_session_inventory(&sessions),
        "d\talpha\na\twork 東京\n"
    );
}

#[test]
fn render_session_inventory_uses_zero_bytes_for_empty_lists() {
    let sessions: Vec<SessionRecord> = Vec::new();
    assert_eq!(render_session_inventory(&sessions), "");
}
