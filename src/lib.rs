pub mod cli;
pub mod config;
pub mod logging;
pub mod picker;
pub mod prompt_integration;
pub mod relay;
pub mod session;
pub mod session_name;
pub mod tmux;
pub mod tmux_version;

const TMUX_REFUSAL: &str = "cannot run from inside tmux; detach or run it from a plain terminal";

/// Reject running stay from inside an existing tmux session.
///
/// The value is passed in so callers and tests can evaluate the policy without
/// mutating the process environment.
///
/// # Errors
///
/// Returns an error when `tmux` is present and non-empty.
pub fn require_not_inside_tmux(tmux: Option<&std::ffi::OsStr>) -> Result<(), String> {
    if tmux.is_some_and(|value| !value.is_empty()) {
        return Err(TMUX_REFUSAL.to_owned());
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn test_global_state_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::require_not_inside_tmux;

    #[test]
    fn allows_absent_or_empty_tmux_values() {
        assert!(require_not_inside_tmux(None).is_ok());
        assert!(require_not_inside_tmux(Some(OsStr::new(""))).is_ok());
    }

    #[test]
    fn rejects_non_empty_tmux_values() {
        let error = require_not_inside_tmux(Some(OsStr::new("/tmp/tmux-123")))
            .expect_err("non-empty TMUX should be rejected");

        assert_eq!(
            error,
            "cannot run from inside tmux; detach or run it from a plain terminal"
        );
    }
}
