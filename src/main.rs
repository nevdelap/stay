use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use stay::{cli::Cli, config::Config, picker, session, tmux, tmux::Tmux, tmux_version};

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
    if cli.prompt_integration {
        writeln!(io::stdout(), "prompt integration is not yet implemented")
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(0);
    }

    let unimplemented_flags = [
        (cli.log_path.is_some(), "-L"),
        (cli.truncate, "-t"),
        (cli.ansi_stripped, "-s"),
        (cli.read_only, "-r"),
        (cli.low_priority, "-l"),
        (cli.pass_through, "-p"),
    ]
    .into_iter()
    .filter_map(|(active, flag)| active.then_some(flag))
    .collect::<Vec<_>>();
    if !unimplemented_flags.is_empty() {
        return Err(format!(
            "{} not yet implemented",
            unimplemented_flags.join(", ")
        ));
    }

    tmux_version::check_installed()?;
    let tmux = Tmux::production();
    let Some(session_name) = cli.session_name.as_deref() else {
        if io::stdout().is_terminal() {
            let config = Config::load()?;
            return picker::run(&tmux, &config);
        }
        let sessions = tmux.list_sessions()?;
        let inventory = tmux::render_session_inventory(&sessions);
        write!(io::stdout(), "{inventory}")
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(0);
    };

    if cli.kill {
        session::kill_session(&tmux, session_name)?;
        return Ok(0);
    }

    let config = Config::load()?;
    if cli.force_recreate {
        session::force_recreate_session(
            &tmux,
            &config,
            session_name,
            cli.cwd.as_deref(),
            &cli.command,
        )?;
        return Ok(0);
    }

    let session_exists = tmux
        .list_sessions()?
        .iter()
        .any(|session| session.name == session_name);
    if session_exists {
        return session::attach_session(&tmux, &config, session_name, &cli.command);
    }

    session::create_session(
        &tmux,
        &config,
        session_name,
        cli.cwd.as_deref(),
        &cli.command,
    )?;
    Ok(0)
}
