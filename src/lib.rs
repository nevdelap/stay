pub mod cli;
pub mod config;
pub mod picker;
pub mod relay;
pub mod session;
pub mod session_name;
pub mod tmux;
pub mod tmux_version;

#[cfg(test)]
pub(crate) fn test_global_state_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
