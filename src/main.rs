use std::process::ExitCode;

mod cli;
pub mod config;
mod tmux_version;

fn main() -> ExitCode {
    match tmux_version::check_installed()
        .and_then(|()| cli::Cli::parse_args(std::env::args()).map_err(|error| error.to_string()))
    {
        Ok(cli) => {
            if cli.prompt_integration {
                println!("prompt integration is not yet implemented");
            } else {
                println!("session orchestration is not yet implemented");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("stay: {error}");
            ExitCode::FAILURE
        }
    }
}
