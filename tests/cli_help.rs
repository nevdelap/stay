use std::process::Command;

#[test]
fn help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--help")
        .output()
        .expect("run stay --help");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: stay"));
}
