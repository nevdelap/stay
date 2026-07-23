use std::process::Stdio;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use stay::tmux::Tmux;

fn unique_namespace() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    format!("stay-test-{nanos}")
}

struct ServerGuard {
    tmux: Tmux,
}

impl ServerGuard {
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

fn create_sleeping_session(tmux: &Tmux, name: &str) {
    let status = tmux
        .command(["new-session", "-d", "-s", name, "--", "sleep", "10"])
        .status()
        .expect("start test session");
    assert!(status.success(), "tmux failed to create {name}");
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
