use std::io::{self, Write};
use std::process::ExitCode;

use stay::{cli::Cli, config::Config, session, tmux::Tmux, tmux_version};

fn main() -> ExitCode {
    match tmux_version::check_installed()
        .and_then(|()| Cli::parse_args(std::env::args()).map_err(|error| error.to_string()))
    {
        Ok(cli) => match dispatch(&cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                let _ = writeln!(io::stderr(), "stay: {error}");
                ExitCode::FAILURE
            }
        },
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

    let Some(session_name) = cli.session_name.as_deref() else {
        writeln!(io::stdout(), "session orchestration is not yet implemented")
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(());
    };

    let config = Config::load()?;
    let tmux = Tmux::production();
    session::create_session(
        &tmux,
        &config,
        session_name,
        cli.cwd.as_deref(),
        &cli.command,
    )
}
