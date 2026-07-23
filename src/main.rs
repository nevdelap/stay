use std::process::ExitCode;

pub mod config;
mod tmux_version;

fn main() -> ExitCode {
    match tmux_version::check_installed() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stay: {error}");
            ExitCode::FAILURE
        }
    }
}
