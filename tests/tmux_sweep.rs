#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nix::unistd::Uid;
use stay::tmux::sweep_orphaned_test_servers;

mod support;
use support::{ScopedEnvironment, TempPath, TestEnvironment};

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
    let environment = TestEnvironment::new();
    let _home = ScopedEnvironment::set("HOME", environment.home());
    let _config = ScopedEnvironment::set("XDG_CONFIG_HOME", environment.config());
    let _tmux = ScopedEnvironment::set("TMUX", "");
    let tmpdir = TempPath::short_directory();
    let _tmpdir = ScopedEnvironment::set("TMUX_TMPDIR", tmpdir.path());
    let live = unique_namespace("stay-test-sweep-live");
    let dead = unique_namespace("stay-test-sweep-dead");
    let untouched = unique_namespace("stay-user-sweep");

    // The command that creates the live server exits without a guard; this
    // leaves the server behind in the same way a SIGKILLed test process does.
    create_session(&live);
    create_session(&dead);
    assert!(socket_path(&live).starts_with(tmpdir.path()));
    assert!(socket_path(&dead).starts_with(tmpdir.path()));
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
    // kill-server stops the server but leaves its socket file behind, and this
    // control namespace sits outside the sweeper's prefix, so nothing else
    // reaps it. Unlink it here so the test cleans up after itself rather than
    // relying only on the per-run TMUX_TMPDIR removal.
    let _ = std::fs::remove_file(socket_path(&untouched));
}

#[test]
fn sweep_skips_an_unresponsive_matching_socket() {
    let _lock = sweep_test_lock();
    let environment = TestEnvironment::new();
    let _home = ScopedEnvironment::set("HOME", environment.home());
    let _config = ScopedEnvironment::set("XDG_CONFIG_HOME", environment.config());
    let _tmux = ScopedEnvironment::set("TMUX", "");
    let tmpdir = TempPath::short_directory();
    let _tmpdir = ScopedEnvironment::set("TMUX_TMPDIR", tmpdir.path());
    let namespace = unique_namespace("stay-test-unresp");
    let path = socket_path(&namespace);
    let socket_parent = path.parent().expect("socket parent");
    std::fs::create_dir_all(socket_parent).expect("create socket parent");
    std::fs::set_permissions(socket_parent, std::fs::Permissions::from_mode(0o700))
        .expect("restrict socket parent");
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
}

#[test]
fn temporary_directory_is_removed_during_unwinding() {
    // Force a panic while the guard is still live and confirm Drop removes the
    // directory as the stack unwinds - the property the guard exists for. A
    // plain block-scope drop (the previous version of this test) exercises only
    // the normal-return path, not unwinding, so it never proved this.
    let captured: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&captured);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let directory = TempPath::directory("stay-test-cleanup");
        let path = directory.path().to_owned();
        std::fs::write(path.join("fixture"), b"fixture").expect("write fixture");
        assert!(path.exists(), "fixture must exist before the panic");
        *sink.lock().expect("record path before panic") = Some(path);
        panic!("force unwinding through the guard's scope");
    }));

    assert!(result.is_err(), "closure must panic so the stack unwinds");
    let path = captured
        .lock()
        .expect("read recorded path")
        .take()
        .expect("path was recorded before the panic");
    assert!(
        !path.exists(),
        "TempPath must be removed while a panic unwinds through its scope"
    );
}
