use std::io::{self, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use stay::{cli::Cli, config::Config, session, tmux, tmux::Tmux, tmux_version};

fn main() -> ExitCode {
    if let Err(error) = tmux_version::check_installed() {
        let _ = writeln!(io::stderr(), "stay: {error}");
        return ExitCode::FAILURE;
    }

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
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "stay: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: &Cli) -> Result<(), String> {
    if cli.prompt_integration {
        writeln!(io::stdout(), "prompt integration is not yet implemented")
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(());
    }

    let tmux = Tmux::production();
    let Some(session_name) = cli.session_name.as_deref() else {
        let sessions = tmux.list_sessions()?;
        let inventory = tmux::render_session_inventory(&sessions);
        write!(io::stdout(), "{inventory}")
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(());
    };

    if cli.kill {
        return session::kill_session(&tmux, session_name);
    }

    let config = Config::load()?;
    if cli.force_recreate {
        return session::force_recreate_session(
            &tmux,
            &config,
            session_name,
            cli.cwd.as_deref(),
            &cli.command,
        );
    }

    session::create_session(
        &tmux,
        &config,
        session_name,
        cli.cwd.as_deref(),
        &cli.command,
    )
}
