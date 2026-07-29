use std::fs;
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
    assert_eq!(String::from_utf8_lossy(&output.stderr), "stay 0.0.25\n");
}

#[test]
fn shell_integration_subcommand_matches_global_prompt_flag() {
    let global = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--prompt-integration")
        .env_remove("TMUX")
        .output()
        .expect("run stay --prompt-integration");
    let subcommand = Command::new(env!("CARGO_BIN_EXE_stay"))
        .args(["shell-integration"])
        .env_remove("TMUX")
        .output()
        .expect("run stay shell-integration");

    assert!(global.status.success());
    assert!(subcommand.status.success());
    assert!(global.stderr.is_empty());
    assert!(subcommand.stderr.is_empty());
    assert_eq!(subcommand.stdout, global.stdout);
}

#[test]
fn prompt_integration_prints_a_snippet_and_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--prompt-integration")
        .env_remove("TMUX")
        .output()
        .expect("run stay --prompt-integration");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty());
    assert!(stdout.contains("stay_prompt_segment"));
}

#[test]
fn prompt_integration_snippet_is_valid_shell_and_reflects_the_session_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--prompt-integration")
        .env_remove("TMUX")
        .output()
        .expect("run stay --prompt-integration");
    assert!(output.status.success());

    let path = std::env::temp_dir().join(format!(
        "stay-prompt-integration-snippet-{}.sh",
        std::process::id()
    ));
    fs::write(&path, &output.stdout).expect("write snippet to a temp file");

    for shell in ["sh", "bash"] {
        let script = format!(
            ". {} && printf 'segment=[%s]\\n' \"$(stay_prompt_segment)\"",
            path.display()
        );

        let unset = Command::new(shell)
            .args(["-c", &script])
            .env_remove("STAY_SESSION_NAME")
            .output()
            .unwrap_or_else(|error| panic!("run snippet under {shell}: {error}"));
        assert!(
            unset.status.success(),
            "{shell} rejected the snippet: {}",
            String::from_utf8_lossy(&unset.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&unset.stdout),
            "segment=[]\n",
            "{shell} with STAY_SESSION_NAME unset"
        );

        let set = Command::new(shell)
            .args(["-c", &script])
            .env("STAY_SESSION_NAME", "work")
            .output()
            .unwrap_or_else(|error| panic!("run snippet under {shell}: {error}"));
        assert!(
            set.status.success(),
            "{shell} rejected the snippet: {}",
            String::from_utf8_lossy(&set.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&set.stdout),
            "segment=[[work] ]\n",
            "{shell} with STAY_SESSION_NAME=work"
        );
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn prompt_integration_snippet_reflects_the_session_name_in_a_zsh_prompt() {
    // zsh isn't a hard dependency of stay itself, only of this regression
    // test for R001 (a default zsh config never expands a `PS1` command
    // substitution without `setopt PROMPT_SUBST`, so the snippet's zsh
    // instructions need to be exercised in an actual zsh, not just sh/bash).
    // CI installs zsh explicitly and every macOS mac-qcheck target ships it,
    // but a bare dev machine might not have it, so skip rather than fail.
    if Command::new("zsh").arg("--version").output().is_err() {
        eprintln!("zsh not found on PATH; skipping zsh prompt-integration regression test");
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_stay"))
        .arg("--prompt-integration")
        .env_remove("TMUX")
        .output()
        .expect("run stay --prompt-integration");
    assert!(output.status.success());

    let path = std::env::temp_dir().join(format!(
        "stay-prompt-integration-snippet-zsh-{}.sh",
        std::process::id()
    ));
    fs::write(&path, &output.stdout).expect("write snippet to a temp file");

    // `print -P` expands a string the same way zsh would expand PS1, without
    // needing an interactive prompt.
    let script = format!(
        ". {} && PS1='$(stay_prompt_segment)' && print -P \"$PS1\"",
        path.display()
    );

    let without_promptsubst = Command::new("zsh")
        .args(["-c", &script])
        .env("STAY_SESSION_NAME", "work")
        .output()
        .expect("run snippet under zsh without PROMPT_SUBST");
    assert!(without_promptsubst.status.success());
    assert_eq!(
        String::from_utf8_lossy(&without_promptsubst.stdout).trim_end(),
        "$(stay_prompt_segment)",
        "without PROMPT_SUBST, zsh must not expand the command substitution \
         (documents the bug the snippet's instructions warn zsh users about)"
    );

    let with_promptsubst_script = format!("setopt PROMPT_SUBST; {script}");
    let with_promptsubst = Command::new("zsh")
        .args(["-c", &with_promptsubst_script])
        .env("STAY_SESSION_NAME", "work")
        .output()
        .expect("run snippet under zsh with PROMPT_SUBST");
    assert!(with_promptsubst.status.success());
    assert_eq!(
        String::from_utf8_lossy(&with_promptsubst.stdout).trim_end(),
        "[work]",
        "with the snippet's documented `setopt PROMPT_SUBST`, zsh must \
         expand the session name into the prompt"
    );

    let _ = fs::remove_file(&path);
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
