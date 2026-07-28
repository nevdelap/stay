use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};

use crate::session_name::parse_session_name;

/// Command-line arguments for the session lifecycle commands.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "stay", version, about = "Persistent tmux sessions")]
pub struct Cli {
    /// The explicit session command, or no command for the picker.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Print the shell prompt-integration snippet.
    #[arg(long, global = true)]
    pub prompt_integration: bool,

    /// Don't use the terminal's alternate screen for the picker.
    ///
    /// The picker normally probes the terminal and uses the alternate screen
    /// only when it is actually supported. This flag forces the picker to
    /// draw on the main screen instead. It is picker-only.
    #[arg(long, global = true)]
    pub no_alt_screen: bool,
}

/// Explicit scripting and session lifecycle commands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// List sessions, optionally as stable JSON.
    List {
        /// Emit the machine-readable JSON listing.
        #[arg(long)]
        json: bool,
    },

    /// Create a new session.
    Create {
        /// New session name.
        #[arg(value_name = "SESSION", value_parser = parse_session_name)]
        session_name: String,

        /// Command to run in the new session.
        #[arg(value_name = "COMMAND", num_args = 0..)]
        command: Vec<String>,

        /// Working directory for the new session.
        #[arg(short = 'c', long = "cwd", value_name = "DIR")]
        cwd: Option<String>,

        /// Kill and recreate an existing session.
        #[arg(short = 'f', long = "force-recreate")]
        force_recreate: bool,
    },

    /// Attach to an existing session.
    Attach {
        /// Existing session name.
        #[arg(value_name = "SESSION", value_parser = parse_session_name)]
        session_name: String,

        /// Log output to FILE.
        #[arg(short = 'l', long = "log", value_name = "FILE")]
        log_path: Option<String>,

        /// Truncate the log file before writing.
        #[arg(short = 't', long = "truncate")]
        truncate: bool,

        /// Capture ANSI escape sequences in the log.
        #[arg(long = "raw")]
        raw: bool,

        /// Attach read-only to the session.
        #[arg(short = 'r', long = "read-only")]
        read_only: bool,

        /// Attach at low priority to the session.
        #[arg(short = 'L', long = "low-priority")]
        low_priority: bool,

        /// Pass stdin through to the session.
        #[arg(short = 'p', long = "pass-through")]
        pass_through: bool,
    },

    /// Kill an existing session.
    Kill {
        /// Existing session name.
        #[arg(value_name = "SESSION", value_parser = parse_session_name)]
        session_name: String,
    },
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
        if self.no_alt_screen && self.command.is_some() {
            return Err(Self::conflict(
                "--no-alt-screen only applies when opening the picker (without a subcommand)",
            ));
        }
        if self.prompt_integration && (self.command.is_some() || self.no_alt_screen) {
            return Err(Self::conflict(
                "--prompt-integration is mutually exclusive with all other flags and subcommands",
            ));
        }

        if let Some(Command::Attach {
            log_path,
            truncate,
            raw,
            read_only,
            low_priority,
            pass_through,
            ..
        }) = self.command.as_ref()
        {
            if *truncate && log_path.is_none() {
                return Err(Self::conflict("-t/--truncate requires -l/--log"));
            }
            if *raw && log_path.is_none() {
                return Err(Self::conflict("--raw requires -l/--log"));
            }
            // -p never calls attach-session, so every other attach modifier
            // is exclusive with it, not just -r: -L/--low-priority and
            // -l/--log (which -t/--truncate and --raw both require anyway,
            // so checking log_path alone also covers those two) would
            // otherwise silently do nothing under -p.
            if *pass_through && *read_only {
                return Err(Self::conflict(
                    "-p/--pass-through conflicts with -r/--read-only",
                ));
            }
            if *pass_through && *low_priority {
                return Err(Self::conflict(
                    "-p/--pass-through conflicts with -L/--low-priority",
                ));
            }
            if *pass_through && log_path.is_some() {
                return Err(Self::conflict("-p/--pass-through conflicts with -l/--log"));
            }
        }

        Ok(())
    }

    fn conflict(message: &str) -> clap::Error {
        Self::command().error(ErrorKind::ArgumentConflict, message)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::parse_args(args.iter().copied())
    }

    #[test]
    fn subcommands_parse() {
        assert!(matches!(
            parse(&["stay", "list"]).unwrap().command,
            Some(Command::List { json: false })
        ));
        assert!(matches!(
            parse(&["stay", "list", "--json"]).unwrap().command,
            Some(Command::List { json: true })
        ));
        assert!(matches!(
            parse(&["stay", "create", "work"]).unwrap().command,
            Some(Command::Create { .. })
        ));
        assert!(matches!(
            parse(&["stay", "attach", "work"]).unwrap().command,
            Some(Command::Attach { .. })
        ));
        assert!(matches!(
            parse(&["stay", "kill", "work"]).unwrap().command,
            Some(Command::Kill { .. })
        ));
        assert!(parse(&["stay"]).unwrap().command.is_none());
    }

    #[test]
    fn old_flat_forms_are_rejected() {
        for args in [
            &["stay", "work"][..],
            &["stay", "-k", "work"][..],
            &["stay", "-f", "work"][..],
            &["stay", "work", "echo", "hi"][..],
        ] {
            assert!(parse(args).is_err(), "accepted old form {args:?}");
        }
    }

    #[test]
    fn attach_modifiers_parse_with_task_027_spellings() {
        let Some(Command::Attach {
            log_path,
            truncate,
            raw,
            low_priority,
            ..
        }) = parse(&[
            "stay", "attach", "work", "-l", "out.log", "-t", "--raw", "-L",
        ])
        .unwrap()
        .command
        else {
            panic!("expected attach command");
        };
        assert_eq!(log_path.as_deref(), Some("out.log"));
        assert!(truncate);
        assert!(raw);
        assert!(low_priority);
    }

    #[test]
    fn create_options_can_follow_the_trailing_command() {
        let Some(Command::Create {
            command,
            cwd,
            force_recreate,
            ..
        }) = parse(&["stay", "create", "work", "sleep", "10", "-c", "/tmp", "-f"])
            .unwrap()
            .command
        else {
            panic!("expected create command");
        };
        assert_eq!(command, ["sleep", "10"]);
        assert_eq!(cwd.as_deref(), Some("/tmp"));
        assert!(force_recreate);
    }

    #[test]
    fn attach_log_validation_names_the_new_flags() {
        for flag in ["-t", "--raw"] {
            let error = parse(&["stay", "attach", "work", flag])
                .unwrap_err()
                .to_string();
            assert!(error.contains(flag));
            assert!(error.contains("-l/--log"));
        }
    }

    #[test]
    fn pass_through_conflicts_with_every_other_attach_modifier() {
        for args in [
            &["stay", "attach", "work", "-p", "-r"][..],
            &["stay", "attach", "work", "-p", "-L"][..],
            &["stay", "attach", "work", "-p", "-l", "out.log"][..],
        ] {
            let error = parse(args).unwrap_err().to_string();
            assert!(error.contains("-p/--pass-through"), "{error}");
        }

        assert!(parse(&["stay", "attach", "work", "-p"]).is_ok());
    }

    #[test]
    fn picker_only_and_prompt_flags_are_validated() {
        let error = parse(&["stay", "list", "--no-alt-screen"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--no-alt-screen"));
        assert!(error.contains("without a subcommand"));

        let error = parse(&["stay", "--prompt-integration", "list"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--prompt-integration"));

        assert!(parse(&["stay", "--no-alt-screen"]).unwrap().no_alt_screen);
        assert!(
            parse(&["stay", "--prompt-integration"])
                .unwrap()
                .prompt_integration
        );
    }

    #[test]
    fn every_subcommand_validates_session_names_during_parsing() {
        for args in [
            &["stay", "create", "bad.name"][..],
            &["stay", "attach", "bad.name"][..],
            &["stay", "kill", "bad.name"][..],
        ] {
            let error = parse(args).unwrap_err().to_string();
            assert!(error.contains("disallowed character '.'"), "{error}");
        }
    }

    #[test]
    fn help_lists_subcommands_and_modifier_shapes() {
        let help = Cli::command().render_help().to_string();
        for command in ["list", "create", "attach", "kill"] {
            assert!(help.contains(command), "help omitted {command}");
        }
        let mut command = Cli::command();
        let attach = command
            .find_subcommand_mut("attach")
            .expect("attach subcommand")
            .render_help()
            .to_string();
        for flag in [
            "-l, --log",
            "-t, --truncate",
            "--raw",
            "-r, --read-only",
            "-L, --low-priority",
            "-p, --pass-through",
        ] {
            assert!(attach.contains(flag), "attach help omitted {flag}");
        }
    }
}
