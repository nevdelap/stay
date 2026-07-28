use std::fmt;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

// The tmux CHANGES files (and, for the two format variables they don't name
// individually, the upstream git history) establish the feature floor used
// here:
// - `remain-on-exit` (the option) dates to 0.9: "Zombie windows... may be
//   set for a window with the new 'remain-on-exit' option" (CHANGES FROM 0.8
//   TO 0.9, 2009) — far below every other requirement below.
// - `pane_dead_status` (format variable) was added in 2.0 (commit 7a0c94b2,
//   "Add pane_dead_status for exit status of dead panes", 2014-12-09); a
//   later, non-blocking refinement in 2.7 (CHANGES FROM 2.6 TO 2.7: "Show
//   exit status and time in the remain-on-exit pane text") changed only when
//   it's considered ready, not its introduction version.
// - `ignore-size` (client flag) was added in 3.2 (CHANGES FROM 3.1c TO 3.2:
//   "This separates the read-only flag from 'ignore size' behaviour (new
//   ignore-size flag)").
// - `pane_dead_time` (format variable) was added in 3.3, alongside
//   remain-on-exit-format (commit a3d92093 / CHANGES FROM 3.2a TO 3.3: "Add
//   remain-on-exit-format to set text shown when pane is dead"), making 3.3
//   — not 3.2 — the highest, and therefore minimum, version stay requires.
pub const MINIMUM_TMUX_VERSION: Version = Version { major: 3, minor: 3 };

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// Verifies that a recent enough tmux is available on `PATH`.
///
/// # Errors
///
/// Returns an error when tmux is missing, cannot be run, times out, or
/// reports a version lower than [`MINIMUM_TMUX_VERSION`].
pub fn check_installed() -> Result<(), String> {
    let output = run_version_command("tmux", &["-V"], crate::tmux::COMMAND_TIMEOUT)?;
    check_version_output(&output)
}

fn run_version_command(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(arguments)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("tmux is required but was not found on PATH ({error})"))?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("failed to read tmux -V output ({error})"))?;
                if !status.success() {
                    return Err("tmux -V failed; please verify the tmux installation".into());
                }
                return String::from_utf8(output.stdout)
                    .map_err(|_| "tmux -V returned invalid UTF-8".into());
            }
            Ok(None) if Instant::now() >= deadline => {
                terminate(&mut child);
                return Err("tmux -V timed out after 2 seconds; tmux may be unresponsive".into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                terminate(&mut child);
                return Err(format!("failed while waiting for tmux -V ({error})"));
            }
        }
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn parse_version(output: &str) -> Result<Version, String> {
    let mut words = output.split_whitespace();
    let prefix = words.next();
    let version_token = words.next();
    let version_token = match (prefix, version_token) {
        (Some("tmux"), Some(version)) => version,
        (Some(prefix), Some(version)) if prefix.starts_with("tmux-") => {
            prefix.strip_prefix("tmux-").unwrap_or(version)
        }
        _ => return Err(format!("could not parse tmux version from {output:?}")),
    };
    let (major, minor) = version_token
        .split_once('.')
        .ok_or_else(|| format!("could not parse tmux version from {output:?}"))?;
    let minor = minor.trim_end_matches(|character: char| !character.is_ascii_digit());
    let version = Version {
        major: major
            .parse()
            .map_err(|_| format!("could not parse tmux version from {output:?}"))?,
        minor: minor
            .parse()
            .map_err(|_| format!("could not parse tmux version from {output:?}"))?,
    };
    Ok(version)
}

fn check_version_output(output: &str) -> Result<(), String> {
    let version = parse_version(output)?;
    if version < MINIMUM_TMUX_VERSION {
        return Err(format!(
            "tmux {MINIMUM_TMUX_VERSION} or newer is required (found {version})"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_normal_and_suffix_versions() {
        assert_eq!(
            parse_version("tmux 3.2"),
            Ok(Version { major: 3, minor: 2 })
        );
        assert_eq!(
            parse_version("tmux 3.6a"),
            Ok(Version { major: 3, minor: 6 })
        );
    }

    #[test]
    fn rejects_malformed_output() {
        assert!(parse_version("not tmux").is_err());
        assert!(parse_version("tmux three.two").is_err());
        assert!(parse_version("").is_err());
    }

    #[test]
    fn enforces_the_feature_floor() {
        assert!(check_version_output("tmux 3.1").is_err());
        assert!(check_version_output("tmux 3.2").is_err());
        assert!(check_version_output("tmux 3.3").is_ok());
        assert!(check_version_output("tmux 4.0").is_ok());
    }

    #[test]
    fn reports_missing_tmux() {
        let error = run_version_command(
            "stay-command-that-does-not-exist",
            &[],
            Duration::from_secs(1),
        )
        .expect_err("missing tmux should fail");
        assert!(error.contains("tmux is required"));
    }

    #[test]
    fn kills_a_wedged_version_probe() {
        let error = run_version_command("sh", &["-c", "sleep 1"], Duration::from_millis(20))
            .expect_err("wedged probe should time out");
        assert!(error.contains("timed out"));
    }
}
