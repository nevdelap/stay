#![cfg(unix)]

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nix::unistd::Uid;
use stay::tmux::sweep_orphaned_test_servers;

fn sweep_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn unique_namespace(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    format!("{prefix}-{}-{nanos}", std::process::id())
}

fn tmux(namespace: &str, arguments: &[&str]) -> Output {
    Command::new("tmux")
        .args(["-L", namespace])
        .args(arguments)
        .output()
        .expect("run tmux")
}

fn create_session(namespace: &str) {
    let output = tmux(
        namespace,
        &["new-session", "-d", "-s", "orphan", "--", "sleep", "30"],
    );
    assert!(
        output.status.success(),
        "tmux failed to create {namespace}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_missing_server(namespace: &str) {
    let output = tmux(namespace, &["list-sessions"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no server running")
            || (stderr.contains("error connecting")
                && stderr.contains("No such file or directory")),
        "expected {namespace} to be gone, got: {stderr}"
    );
}

fn socket_path(namespace: &str) -> PathBuf {
    let root = std::env::var_os("TMUX_TMPDIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    root.join(format!("tmux-{}/{}", Uid::current().as_raw(), namespace))
}

#[test]
fn sweep_reaps_live_and_dead_test_servers_but_not_other_namespaces() {
    let _lock = sweep_test_lock();
    let live = unique_namespace("stay-test-sweep-live");
    let dead = unique_namespace("stay-test-sweep-dead");
    let untouched = unique_namespace("stay-user-sweep");

    // The command that creates the live server exits without a guard; this
    // leaves the server behind in the same way a SIGKILLed test process does.
    create_session(&live);
    create_session(&dead);
    let killed = tmux(&dead, &["kill-server"]);
    assert!(killed.status.success());
    create_session(&untouched);

    let report = sweep_orphaned_test_servers().expect("sweep test servers");

    assert!(report.killed_live.contains(&live));
    assert!(report.removed_dead.contains(&dead));
    assert_missing_server(&live);
    assert_missing_server(&dead);
    assert!(
        tmux(&untouched, &["list-sessions"]).status.success(),
        "sweep touched a non-stay-test namespace"
    );

    let cleaned = tmux(&untouched, &["kill-server"]);
    assert!(cleaned.status.success());
}

#[test]
fn sweep_skips_an_unresponsive_matching_socket() {
    let _lock = sweep_test_lock();
    let namespace = unique_namespace("stay-test-sweep-unresponsive");
    let path = socket_path(&namespace);
    let listener = UnixListener::bind(&path).expect("bind unresponsive tmux socket");
    let accept_thread = std::thread::spawn(move || {
        if let Ok((_stream, _)) = listener.accept() {
            std::thread::sleep(Duration::from_secs(3));
        }
    });

    let report = sweep_orphaned_test_servers().expect("sweep unresponsive socket");

    assert!(!report.killed_live.contains(&namespace));
    assert!(!report.removed_dead.contains(&namespace));
    accept_thread.join().expect("join socket listener");
    std::fs::remove_file(path).expect("remove unresponsive socket fixture");
}
