use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use stay::{
    cli::{Cli, Command},
    config::Config,
    picker, require_not_inside_tmux, session,
    tmux::{self, Tmux},
    tmux_version,
};

fn main() -> ExitCode {
    let cli = match Cli::parse_args(std::env::args()) {
        Ok(cli) => cli,
        Err(error) => {
            let success = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let _ = write!(io::stderr(), "{error}");
            return if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    };

    match dispatch(&cli) {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            let _ = writeln!(io::stderr(), "stay: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: &Cli) -> Result<u8, String> {
    require_not_inside_tmux(std::env::var_os("TMUX").as_deref())?;

    if cli.prompt_integration {
        writeln!(io::stdout(), "prompt integration is not yet implemented")
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(0);
    }

    reject_unimplemented_attach_options(cli)?;
    tmux_version::check_installed()?;
    let tmux = Tmux::production();
    match cli.command.as_ref() {
        None => {
            if !io::stdout().is_terminal() {
                return Err(
                    "the interactive picker requires a terminal; use `stay list`".to_owned(),
                );
            }
            let config = Config::load()?;
            let screen = if cli.no_alt_screen {
                picker::ScreenPreference::ForceMainScreen
            } else {
                picker::ScreenPreference::Auto
            };
            picker::run(&tmux, &config, screen)
        }
        Some(Command::List { json }) => {
            let sessions = tmux.list_sessions()?;
            let output = if *json {
                tmux::render_session_json(&sessions)
            } else {
                tmux::render_session_inventory(&sessions, io::stdout().is_terminal())
            };
            write!(io::stdout(), "{output}")
                .map_err(|error| format!("failed to write stdout: {error}"))?;
            Ok(0)
        }
        Some(Command::Create {
            session_name,
            command,
            cwd,
            force_recreate,
        }) => {
            let config = Config::load()?;
            if *force_recreate {
                session::force_recreate_session(
                    &tmux,
                    &config,
                    session_name,
                    cwd.as_deref(),
                    command,
                )?;
                return Ok(0);
            }
            if tmux
                .list_sessions()?
                .iter()
                .any(|session| session.name == *session_name)
            {
                return Err(format!(
                    "session {session_name:?} already exists; use -f/--force-recreate"
                ));
            }
            session::create_session(&tmux, &config, session_name, cwd.as_deref(), command)?;
            Ok(0)
        }
        Some(Command::Attach {
            session_name,
            read_only,
            low_priority,
            ..
        }) => {
            if !tmux
                .list_sessions()?
                .iter()
                .any(|session| session.name == *session_name)
            {
                return Err(format!("session {session_name:?} does not exist"));
            }
            let config = Config::load()?;
            session::attach_session(&tmux, &config, session_name, &[], *read_only, *low_priority)
        }
        Some(Command::Kill { session_name }) => {
            session::kill_session(&tmux, session_name)?;
            Ok(0)
        }
    }
}

fn reject_unimplemented_attach_options(cli: &Cli) -> Result<(), String> {
    let Some(Command::Attach {
        log_path,
        truncate,
        raw,
        pass_through,
        ..
    }) = cli.command.as_ref()
    else {
        return Ok(());
    };

    let unimplemented_flags = [
        (log_path.is_some(), "-l/--log"),
        (*truncate, "-t/--truncate"),
        (*raw, "--raw"),
        (*pass_through, "-p/--pass-through"),
    ]
    .into_iter()
    .filter_map(|(active, flag)| active.then_some(flag))
    .collect::<Vec<_>>();
    if unimplemented_flags.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} not yet implemented",
            unimplemented_flags.join(", ")
        ))
    }
}
