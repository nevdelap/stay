use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use stay::{
    cli::{Cli, Command},
    config::Config,
    logging, picker, prompt_integration, require_not_inside_tmux, session,
    session_store::{CommitStatus, SessionDefinition, SessionStore, StoreError},
    shell_integration,
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
            if success {
                let _ = write!(io::stdout(), "{error}");
            } else {
                let _ = write!(io::stderr(), "{error}");
            }
            return ExitCode::from(if success { 0 } else { 2 });
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
    if let Some(Command::RawLogWriter { path }) = cli.command.as_ref() {
        return logging::run_raw_log_writer(path);
    }
    if cli.prompt_integration {
        write!(io::stdout(), "{}", prompt_integration::snippet())
            .map_err(|error| format!("failed to write stdout: {error}"))?;
        return Ok(0);
    }

    if let Some(Command::ShellIntegration { s_alias }) = cli.command.as_ref() {
        shell_integration::run(*s_alias)?;
        return Ok(0);
    }

    require_not_inside_tmux(std::env::var_os("TMUX").as_deref())?;

    let tmux = Tmux::production();
    tmux_version::check_installed()?;
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
            let store = SessionStore::open_default()?;
            let saved = store.load().map_err(|error| store_error(&error))?;
            let sessions = tmux::merge_saved_sessions(tmux.list_sessions()?, &saved);
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
            attach,
            read_only,
            low_priority,
        }) => dispatch_create(
            &tmux,
            session_name,
            command,
            cwd.as_deref(),
            *force_recreate,
            *attach,
            session::AttachOptions {
                read_only: *read_only,
                low_priority: *low_priority,
                ..session::AttachOptions::default()
            },
        ),
        Some(Command::Attach {
            session_name,
            read_only,
            low_priority,
            log_path,
            truncate,
            raw,
            pass_through,
        }) => {
            if *pass_through {
                dispatch_pass_through(&tmux, session_name)
            } else {
                dispatch_attach(
                    &tmux,
                    session_name,
                    session::AttachOptions {
                        read_only: *read_only,
                        low_priority: *low_priority,
                        log_path: log_path.as_deref(),
                        truncate: *truncate,
                        raw: *raw,
                    },
                )
            }
        }
        Some(Command::Kill { session_name }) => dispatch_kill(&tmux, session_name),
        Some(Command::ShellIntegration { .. }) => {
            unreachable!("shell integration is dispatched before tmux setup")
        }
        Some(Command::RawLogWriter { .. }) => {
            unreachable!("raw log writer is dispatched before tmux setup")
        }
    }
}

fn dispatch_create(
    tmux: &Tmux,
    session_name: &str,
    command: &[String],
    cwd: Option<&str>,
    force_recreate: bool,
    attach: bool,
    attach_options: session::AttachOptions<'_>,
) -> Result<u8, String> {
    let config = Config::load()?;
    let store = SessionStore::open_default()?;
    let definition = session::resolve_session_definition(&config, session_name, cwd, command)?;
    let mut saved = store.load().map_err(|error| store_error(&error))?;
    let previous = saved.clone();
    let had_saved_definition = saved.contains_key(session_name);
    if force_recreate {
        saved.insert(session_name.to_owned(), definition.clone());
    } else {
        if tmux.has_session(session_name)? {
            return Err(format!(
                "session {session_name:?} already exists; use -f/--force-recreate"
            ));
        }
        if had_saved_definition {
            return Err(format!(
                "session {session_name:?} is saved but not running; use -f/--force-recreate"
            ));
        }
        saved.insert(session_name.to_owned(), definition.clone());
    }
    commit_store(&store, &saved)?;

    let tmux_result = if force_recreate {
        session::force_recreate_session_with_definition(tmux, &config, &definition)
    } else {
        session::create_session_with_definition(tmux, &config, &definition)
    };
    if let Err(error) = tmux_result {
        restore_store_after_tmux_failure(&store, &previous, error)?;
    }

    if attach {
        return session::attach_session(tmux, &config, session_name, &[], attach_options);
    }
    Ok(0)
}

fn dispatch_kill(tmux: &Tmux, session_name: &str) -> Result<u8, String> {
    let store = SessionStore::open_default()?;
    let mut saved = store.load().map_err(|error| store_error(&error))?;
    let live = tmux
        .list_sessions()?
        .into_iter()
        .any(|session| session.name == session_name);
    if live {
        session::kill_session(tmux, session_name)?;
    } else if !saved.contains_key(session_name) {
        session::kill_session(tmux, session_name)?;
        return Ok(0);
    }
    if saved.remove(session_name).is_some() {
        match store.commit(&saved) {
            Ok(CommitStatus::Durable) => {}
            Ok(CommitStatus::Uncertain(message)) => {
                return Err(format!(
                    "session {session_name:?} was killed, but saved-definition removal has uncertain durability: {message}"
                ));
            }
            Err(error) => {
                return Err(format!(
                    "session {session_name:?} was killed but its saved definition could not be removed: {error}"
                ));
            }
        }
    }
    Ok(0)
}

fn commit_store(
    store: &SessionStore,
    sessions: &std::collections::BTreeMap<String, SessionDefinition>,
) -> Result<(), String> {
    match store.commit(sessions) {
        Ok(CommitStatus::Durable) => Ok(()),
        Ok(CommitStatus::Uncertain(message)) => Err(format!(
            "failed to persist session definition: {message}; durability is uncertain"
        )),
        Err(error) => Err(format!("failed to persist session definition: {error}")),
    }
}

fn restore_store_after_tmux_failure(
    store: &SessionStore,
    previous: &std::collections::BTreeMap<String, SessionDefinition>,
    tmux_error: String,
) -> Result<(), String> {
    match store.commit(previous) {
        Ok(CommitStatus::Durable) => Err(tmux_error),
        Ok(CommitStatus::Uncertain(message)) => Err(format!(
            "{tmux_error}; restoring the previous session store completed with uncertain durability: {message}"
        )),
        Err(restore_error) => Err(format!(
            "{tmux_error}; restoring the previous session store also failed: {restore_error}"
        )),
    }
}

fn store_error(error: &StoreError) -> String {
    error.to_string()
}

fn dispatch_attach(
    tmux: &Tmux,
    session_name: &str,
    options: session::AttachOptions<'_>,
) -> Result<u8, String> {
    require_existing_session(tmux, session_name)?;
    let config = Config::load()?;
    session::attach_session(tmux, &config, session_name, &[], options)
}

fn dispatch_pass_through(tmux: &Tmux, session_name: &str) -> Result<u8, String> {
    require_existing_session(tmux, session_name)?;
    session::pass_through(tmux, session_name)?;
    Ok(0)
}

fn require_existing_session(tmux: &Tmux, session_name: &str) -> Result<(), String> {
    if tmux.has_session(session_name)? {
        Ok(())
    } else {
        Err(format!("session {session_name:?} does not exist"))
    }
}
