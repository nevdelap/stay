use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Effective tmux history limit for `history_lines = "unlimited"`.
pub const UNLIMITED_HISTORY_LINES: usize = 1_000_000;
pub const DEFAULT_LOG_CAPTURE_INTERVAL_SECONDS: u64 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub default_command: Option<String>,
    pub detach_key: u8,
    pub copy_mode_key: u8,
    /// Effective tmux history limit; `"unlimited"` means one million lines.
    pub history_lines: usize,
    pub log_capture_interval_seconds: u64,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    default_command: Option<String>,
    detach_key: Option<String>,
    copy_mode_key: Option<String>,
    history_lines: Option<HistoryValue>,
    log_capture_interval_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HistoryValue {
    Number(usize),
    Text(String),
}

impl Config {
    /// Loads the platform-default config file and environment overrides.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, an override
    /// is invalid, or the two configured keys collide.
    pub fn load() -> Result<Self, String> {
        let path = config_path();
        load_from_path_and_env(path.as_deref(), &current_environment())
    }

    /// Loads a config file and the current process environment.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, an override
    /// is invalid, or the two configured keys collide.
    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        load_from_path_and_env(Some(path), &current_environment())
    }
}

fn current_environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("stay/config.toml"))
}

fn non_empty_environment_value<'a>(
    environment: &'a BTreeMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    environment
        .get(key)
        .filter(|value| !value.is_empty())
        .map(String::as_str)
}

fn load_from_path_and_env(
    path: Option<&Path>,
    environment: &BTreeMap<String, String>,
) -> Result<Config, String> {
    let file = match path {
        Some(path) if path.exists() => {
            let contents = fs::read_to_string(path).map_err(|error| {
                format!("failed to read config file {} ({error})", path.display())
            })?;
            toml::from_str::<FileConfig>(&contents).map_err(|error| {
                format!("failed to parse config file {} ({error})", path.display())
            })?
        }
        _ => FileConfig::default(),
    };

    let detach_spec = non_empty_environment_value(environment, "STAY_DETACH_KEY")
        .map(str::to_owned)
        .or(file.detach_key)
        .unwrap_or_else(|| "Ctrl+\\".to_owned());
    let copy_spec = non_empty_environment_value(environment, "STAY_COPY_MODE_KEY")
        .map(str::to_owned)
        .or(file.copy_mode_key)
        .unwrap_or_else(|| "Ctrl+Space".to_owned());
    let detach_key = parse_key_spec(&detach_spec)?;
    let copy_mode_key = parse_key_spec(&copy_spec)?;
    if detach_key == copy_mode_key {
        return Err(format!(
            "detach_key ({detach_spec}) and copy_mode_key ({copy_spec}) resolve to the same control byte"
        ));
    }

    let default_command = non_empty_environment_value(environment, "STAY_CMD")
        .map(str::to_owned)
        .or(file.default_command.filter(|value| !value.is_empty()));
    let history_lines = environment
        .get("STAY_HISTORY_LINES")
        .filter(|value| !value.is_empty())
        .map(|value| parse_history_text(value))
        .or_else(|| file.history_lines.map(|value| parse_history(&value)))
        .transpose()?
        .unwrap_or(10_000);
    let log_capture_interval_seconds = environment
        .get("STAY_LOG_CAPTURE_INTERVAL_SECONDS")
        .filter(|value| !value.is_empty())
        .map(|value| parse_log_capture_interval(value))
        .or_else(|| {
            file.log_capture_interval_seconds
                .map(validate_log_capture_interval)
        })
        .transpose()?
        .unwrap_or(DEFAULT_LOG_CAPTURE_INTERVAL_SECONDS);

    Ok(Config {
        default_command,
        detach_key,
        copy_mode_key,
        history_lines,
        log_capture_interval_seconds,
    })
}

fn parse_log_capture_interval(value: &str) -> Result<u64, String> {
    let value = value.parse::<u64>().map_err(|_| {
        format!("log_capture_interval_seconds must be a positive integer (got {value:?})")
    })?;
    validate_log_capture_interval(value)
}

fn validate_log_capture_interval(value: u64) -> Result<u64, String> {
    if value == 0 {
        Err("log_capture_interval_seconds must be a positive integer".to_owned())
    } else {
        Ok(value)
    }
}

fn parse_history(value: &HistoryValue) -> Result<usize, String> {
    let value = match value {
        HistoryValue::Number(value) => *value,
        HistoryValue::Text(value) if value == "unlimited" => UNLIMITED_HISTORY_LINES,
        HistoryValue::Text(value) => value.parse().map_err(|_| {
            format!("history_lines must be a positive integer or \"unlimited\" (got {value:?})")
        })?,
    };
    if value == 0 {
        Err("history_lines must be a positive integer".to_owned())
    } else {
        Ok(value)
    }
}

fn parse_history_text(value: &str) -> Result<usize, String> {
    parse_history(&HistoryValue::Text(value.to_owned()))
}

fn parse_key_spec(spec: &str) -> Result<u8, String> {
    const ALLOWED: &str = "an ASCII letter, Space, ?, @, [, \\, ], ^, or _";
    let key = spec.strip_prefix("Ctrl+").ok_or_else(|| {
        format!("unsupported key specification {spec:?}; expected Ctrl+ followed by {ALLOWED}")
    })?;
    let byte = match key {
        "Space" => 0,
        // Ctrl+? is DEL, which collides with Backspace on most terminals.
        "?" => 0x7f,
        _ => {
            let characters: Vec<_> = key.chars().collect();
            let character = *characters
                .first()
                .filter(|_| characters.len() == 1)
                .ok_or_else(|| {
                    format!(
                        "unsupported key specification {spec:?}; expected Ctrl+ followed by {ALLOWED}"
                    )
                })?;
            if !(character.is_ascii_alphabetic()
                || matches!(character, '@' | '[' | '\\' | ']' | '^' | '_'))
            {
                return Err(format!(
                    "unsupported key specification {spec:?}; expected Ctrl+ followed by {ALLOWED}"
                ));
            }
            character.to_ascii_uppercase() as u8 & 0x1f
        }
    };
    Ok(byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempPath;

    fn fixture(contents: &str) -> TempPath {
        let path = TempPath::file("stay-config");
        fs::write(&path, contents).expect("write config fixture");
        path
    }

    fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    fn load_file(path: &Path) -> Result<Config, String> {
        load_from_path_and_env(Some(path), &BTreeMap::new())
    }

    #[test]
    fn defaults_leave_the_command_unconfigured_and_use_key_defaults() {
        let config = load_from_path_and_env(None, &env(&[("SHELL", "/bin/zsh")])).unwrap();
        assert_eq!(config.default_command, None);
        assert_eq!(config.detach_key, 0x1c);
        assert_eq!(config.copy_mode_key, 0);
        assert_eq!(config.history_lines, 10_000);
        assert_eq!(config.log_capture_interval_seconds, 5);
    }

    #[test]
    fn defaults_leave_the_command_unconfigured_when_shell_is_unset() {
        let config = load_from_path_and_env(None, &BTreeMap::new()).unwrap();
        assert_eq!(config.default_command, None);
    }

    #[test]
    fn empty_environment_command_behaves_as_unset() {
        let config = load_from_path_and_env(None, &env(&[("STAY_CMD", "")])).unwrap();
        assert_eq!(config.default_command, None);
    }

    #[test]
    fn empty_file_command_behaves_as_unset() {
        let path = fixture("default_command = \"\"\n");
        assert_eq!(load_file(&path).unwrap().default_command, None);
    }

    #[test]
    fn file_values_are_overridden_by_environment() {
        let path = fixture(
            "default_command = \"fish\"\ndetach_key = \"Ctrl+A\"\ncopy_mode_key = \"Ctrl+B\"\nhistory_lines = 42\nlog_capture_interval_seconds = 9\n",
        );
        let config = load_from_path_and_env(
            Some(&path),
            &env(&[
                ("STAY_CMD", "nsh"),
                ("STAY_DETACH_KEY", "Ctrl+C"),
                ("STAY_COPY_MODE_KEY", "Ctrl+D"),
                ("STAY_HISTORY_LINES", "unlimited"),
                ("STAY_LOG_CAPTURE_INTERVAL_SECONDS", "11"),
            ]),
        )
        .unwrap();
        assert_eq!(config.default_command, Some("nsh".to_owned()));
        assert_eq!(config.detach_key, 3);
        assert_eq!(config.copy_mode_key, 4);
        assert_eq!(config.history_lines, UNLIMITED_HISTORY_LINES);
        assert_eq!(config.log_capture_interval_seconds, 11);
    }

    #[test]
    fn log_capture_interval_falls_back_to_the_file_value_and_rejects_zero() {
        let path = fixture("log_capture_interval_seconds = 7\n");
        assert_eq!(load_file(&path).unwrap().log_capture_interval_seconds, 7);
        let path = fixture("log_capture_interval_seconds = 0\n");
        assert!(load_file(&path).is_err());
    }

    #[test]
    fn accepts_string_history_and_rejects_invalid_values() {
        let path = fixture("history_lines = \"unlimited\"\n");
        assert_eq!(
            load_file(&path).unwrap().history_lines,
            UNLIMITED_HISTORY_LINES
        );
        let path = fixture("history_lines = 0\n");
        assert!(load_file(&path).is_err());
    }

    #[test]
    fn rejects_key_collision_and_malformed_toml() {
        let path = fixture("detach_key = \"Ctrl+A\"\ncopy_mode_key = \"Ctrl+A\"\n");
        let error = load_file(&path).unwrap_err();
        assert!(error.contains("detach_key") && error.contains("copy_mode_key"));
        let path = fixture("history_lines = [");
        assert!(load_file(&path).unwrap_err().contains("parse config file"));
    }

    #[test]
    fn parses_control_key_specs() {
        assert_eq!(parse_key_spec("Ctrl+A"), Ok(1));
        assert_eq!(parse_key_spec("Ctrl+\\"), Ok(0x1c));
        assert_eq!(parse_key_spec("Ctrl+Space"), Ok(0));
        assert_eq!(parse_key_spec("Ctrl+?"), Ok(0x7f));
        assert!(
            parse_key_spec("Ctrl+2")
                .unwrap_err()
                .contains("ASCII letter")
        );
        assert!(
            parse_key_spec("Ctrl+;")
                .unwrap_err()
                .contains("ASCII letter")
        );
        assert!(parse_key_spec("Alt+A").is_err());
    }
}
