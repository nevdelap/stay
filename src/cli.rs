use clap::{error::ErrorKind, CommandFactory, Parser};

use crate::session_name::parse_session_name;

/// Command-line arguments for the session lifecycle commands.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "stay", version, about = "Persistent tmux sessions")]
pub struct Cli {
    /// Existing or new session name.
    #[arg(value_name = "SESSION", value_parser = parse_session_name)]
    pub session_name: Option<String>,

    /// Command to run when creating a session.
    #[arg(value_name = "COMMAND", num_args = 0.., trailing_var_arg = true)]
    pub command: Vec<String>,

    /// Working directory for a newly created session.
    #[arg(short = 'c', value_name = "DIR")]
    pub cwd: Option<String>,

    /// Log output to FILE.
    #[arg(short = 'L', value_name = "FILE")]
    pub log_path: Option<String>,

    /// Truncate the log file before writing.
    #[arg(short = 't')]
    pub truncate: bool,

    /// Strip ANSI escape sequences from the log.
    #[arg(short = 's')]
    pub ansi_stripped: bool,

    /// Kill the named session.
    #[arg(short = 'k')]
    pub kill: bool,

    /// Attach read-only to the named session.
    #[arg(short = 'r')]
    pub read_only: bool,

    /// Attach at low priority to the named session.
    #[arg(short = 'l')]
    pub low_priority: bool,

    /// Recreate the named session.
    #[arg(short = 'f')]
    pub force_recreate: bool,

    /// Pass stdin through to the named session.
    #[arg(short = 'p')]
    pub pass_through: bool,

    /// Print the shell prompt-integration snippet.
    #[arg(long)]
    pub prompt_integration: bool,

    /// Don't use the terminal's alternate screen for the picker.
    ///
    /// The picker normally probes the terminal and uses the alternate
    /// screen only when it is actually supported. This flag forces the
    /// picker to draw on the main screen instead — useful for terminals
    /// where the probe is unreliable. Only meaningful when opening the
    /// picker (no session name).
    #[arg(long)]
    pub no_alt_screen: bool,

    /// Force the picker to use the alternate screen, skipping the probe.
    ///
    /// Only meaningful when opening the picker (no session name). Conflicts
    /// with `--no-alt-screen`.
    #[arg(long)]
    pub alt_screen: bool,
}

impl Cli {
    /// Parse arguments and apply stay's cross-argument validation rules.
    ///
    /// # Errors
    ///
    /// Returns a clap error when parsing or validation fails.
    pub fn parse_args<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Self::try_parse_from(args)?;
        cli.validate()?;
        Ok(cli)
    }

    fn validate(&self) -> Result<(), clap::Error> {
        if self.truncate && self.log_path.is_none() {
            return Err(Self::conflict("-t/--truncate requires -L/--log"));
        }
        if self.ansi_stripped && self.log_path.is_none() {
            return Err(Self::conflict("-s/--ansi-stripped requires -L/--log"));
        }

        let action_flags = [
            (self.kill, "-k/--kill"),
            (self.read_only, "-r/--read-only"),
            (self.low_priority, "-l/--low-priority"),
            (self.force_recreate, "-f/--force-recreate"),
            (self.pass_through, "-p/--pass-through"),
        ];
        let active_actions: Vec<_> = action_flags
            .iter()
            .filter_map(|(active, name)| active.then_some(*name))
            .collect();

        if self.kill && active_actions.len() > 1 {
            return Err(Self::conflict(&format!(
                "-k/--kill conflicts with {}",
                active_actions
                    .iter()
                    .copied()
                    .filter(|name| *name != "-k/--kill")
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if self.read_only && self.pass_through {
            return Err(Self::conflict(
                "-r/--read-only conflicts with -p/--pass-through",
            ));
        }
        if self.no_alt_screen && self.alt_screen {
            return Err(Self::conflict(
                "--no-alt-screen conflicts with --alt-screen",
            ));
        }
        // The screen-mode flags only affect the interactive picker, which
        // runs when no session is named; reject them as silently inert
        // anywhere else.
        if (self.no_alt_screen || self.alt_screen)
            && (self.session_name.is_some()
                || !self.command.is_empty()
                || self.cwd.is_some()
                || self.log_path.is_some()
                || !active_actions.is_empty())
        {
            return Err(Self::conflict(
                "--no-alt-screen/--alt-screen only apply when opening the picker (no session name)",
            ));
        }
        if !active_actions.is_empty() && self.session_name.is_none() {
            return Err(Self::conflict(&format!(
                "{} requires a session name",
                active_actions.join(", ")
            )));
        }
        let command_incompatible_actions =
            self.kill || self.read_only || self.low_priority || self.pass_through;
        if command_incompatible_actions && !self.command.is_empty() {
            return Err(Self::conflict(&format!(
                "{} cannot be combined with trailing command words",
                [
                    (self.kill, "-k/--kill"),
                    (self.read_only, "-r/--read-only"),
                    (self.low_priority, "-l/--low-priority"),
                    (self.pass_through, "-p/--pass-through"),
                ]
                .into_iter()
                .filter_map(|(active, name)| active.then_some(name))
                .collect::<Vec<_>>()
                .join(", ")
            )));
        }

        if self.prompt_integration
            && (self.session_name.is_some()
                || !self.command.is_empty()
                || self.cwd.is_some()
                || self.log_path.is_some()
                || self.truncate
                || self.ansi_stripped
                || self.no_alt_screen
                || self.alt_screen
                || !active_actions.is_empty())
        {
            return Err(Self::conflict(
                "--prompt-integration is mutually exclusive with all other flags and positionals",
            ));
        }

        Ok(())
    }

    fn conflict(message: &str) -> clap::Error {
        Self::command().error(ErrorKind::ArgumentConflict, message)
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::parse_args(args.iter().copied())
    }

    #[test]
    fn legal_combinations_parse() {
        for args in [
            &["stay"][..],
            &["stay", "work"],
            &["stay", "work", "sh", "-c", "echo hi"],
            &["stay", "-c", "/tmp", "work"],
            &["stay", "-L", "out.log", "-t", "-s", "work"],
            &["stay", "-k", "work"],
            &["stay", "-r", "work"],
            &["stay", "-l", "work"],
            &["stay", "-f", "work"],
            &["stay", "-f", "work", "sh", "-c", "echo hi"],
            &["stay", "-p", "work"],
            &["stay", "--prompt-integration"],
            &["stay", "--no-alt-screen"],
            &["stay", "--alt-screen"],
        ] {
            assert!(parse(args).is_ok(), "failed to parse {args:?}");
        }

        let cli = parse(&["stay", "work", "sh", "-c", "echo hi"]).unwrap();
        assert_eq!(cli.command, ["sh", "-c", "echo hi"]);

        let cli = parse(&["stay", "--no-alt-screen"]).unwrap();
        assert!(cli.no_alt_screen);

        let cli = parse(&["stay", "--alt-screen"]).unwrap();
        assert!(cli.alt_screen);
    }

    #[test]
    fn required_log_flag_is_named() {
        for flag in ["-t", "-s"] {
            let error = parse(&["stay", flag]).unwrap_err().to_string();
            assert!(error.contains(flag));
            assert!(error.contains("-L/--log"));
        }
    }

    #[test]
    fn action_rules_name_conflicting_flags() {
        let error = parse(&["stay", "-k", "-r", "work"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("-k/--kill"));
        assert!(error.contains("-r/--read-only"));

        let error = parse(&["stay", "-r", "-p", "work"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("-r/--read-only"));
        assert!(error.contains("-p/--pass-through"));
    }

    #[test]
    fn actions_require_session_and_reject_commands() {
        let error = parse(&["stay", "-k"]).unwrap_err().to_string();
        assert!(error.contains("-k/--kill"));
        assert!(error.contains("session name"));

        let error = parse(&["stay", "-p", "work", "echo", "hi"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("-p/--pass-through"));
        assert!(error.contains("trailing command words"));
    }

    #[test]
    fn prompt_integration_is_exclusive() {
        let error = parse(&["stay", "--prompt-integration", "work"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--prompt-integration"));
        assert!(error.contains("positionals"));

        let error = parse(&["stay", "--prompt-integration", "-L", "out.log"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--prompt-integration"));
        assert!(error.contains("all other flags"));
    }

    #[test]
    fn help_lists_the_complete_flag_shape() {
        let help = Cli::command().render_help().to_string();
        for flag in [
            "-c",
            "-L",
            "-t",
            "-s",
            "-k",
            "-r",
            "-l",
            "-f",
            "-p",
            "--prompt-integration",
            "--no-alt-screen",
            "--alt-screen",
        ] {
            assert!(help.contains(flag), "help omitted {flag}");
        }
    }

    #[test]
    fn session_name_is_validated_during_parsing() {
        let error = parse(&["stay", "bad.name"]).unwrap_err().to_string();
        assert!(error.contains("disallowed character '.'"));
        assert!(error.contains("position 3"));
    }

    #[test]
    fn screen_mode_flags_are_mutually_exclusive() {
        let error = parse(&["stay", "--no-alt-screen", "--alt-screen"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--no-alt-screen"));
        assert!(error.contains("--alt-screen"));
    }

    #[test]
    fn screen_mode_flags_are_picker_only() {
        for args in [
            &["stay", "--no-alt-screen", "work"][..],
            &["stay", "--alt-screen", "work"][..],
            &["stay", "--no-alt-screen", "-k", "work"][..],
            &["stay", "--alt-screen", "echo", "hi"][..],
        ] {
            let error = parse(args).unwrap_err().to_string();
            assert!(
                error.contains("--no-alt-screen/--alt-screen"),
                "expected picker-only conflict for {args:?}, got: {error}"
            );
        }
    }
}
