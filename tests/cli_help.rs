use std::process::Command;

#[test]
fn help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--help")
        .env("TMUX", "/tmp/tmux-123/default,1,0")
        .output()
        .expect("run stay --help");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: stay"));
}

#[test]
fn version_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--version")
        .env("TMUX", "/tmp/tmux-123/default,1,0")
        .output()
        .expect("run stay --version");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "stay 0.0.4\n");
}

#[test]
fn refuses_non_help_invocations_inside_tmux() {
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--prompt-integration")
        .env("TMUX", "/tmp/tmux-123/default,1,0")
        .output()
        .expect("run stay --prompt-integration");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "stay: cannot run from inside tmux; detach or run it from a plain terminal\n"
    );
    assert!(output.stdout.is_empty());
}
