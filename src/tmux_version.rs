use std::fmt;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

// The tmux CHANGES files establish the feature floor used here:
// `remain-on-exit` dates to the 0.8-era zombie-window support, and the
// dead-pane exit status/time reporting is documented by the 2.8 release.
// `ignore-size` was added in the 3.2 release (CHANGES FROM 3.1c TO 3.2),
// making 3.2 the highest—and therefore minimum—version required by stay.
pub const MINIMUM_TMUX_VERSION: Version = Version { major: 3, minor: 2 };

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
        assert!(check_version_output("tmux 3.2").is_ok());
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
