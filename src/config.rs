use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const UNLIMITED_HISTORY_LINES: usize = 1_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub default_command: Option<String>,
    pub detach_key: u8,
    pub copy_mode_key: u8,
    pub history_lines: usize,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    default_command: Option<String>,
    detach_key: Option<String>,
    copy_mode_key: Option<String>,
    history_lines: Option<HistoryValue>,
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

    let detach_spec = environment
        .get("STAY_DETACH_KEY")
        .cloned()
        .or(file.detach_key)
        .unwrap_or_else(|| "Ctrl+\\".to_owned());
    let copy_spec = environment
        .get("STAY_COPY_MODE_KEY")
        .cloned()
        .or(file.copy_mode_key)
        .unwrap_or_else(|| "Ctrl+Space".to_owned());
    let detach_key = parse_key_spec(&detach_spec)?;
    let copy_mode_key = parse_key_spec(&copy_spec)?;
    if detach_key == copy_mode_key {
        return Err(format!(
            "detach_key ({detach_spec}) and copy_mode_key ({copy_spec}) resolve to the same control byte"
        ));
    }

    let default_command = environment
        .get("STAY_CMD")
        .cloned()
        .or(file.default_command);
    let history_lines = environment
        .get("STAY_HISTORY_LINES")
        .map(|value| parse_history_text(value))
        .or_else(|| file.history_lines.map(|value| parse_history(&value)))
        .transpose()?
        .unwrap_or(10_000);

    Ok(Config {
        default_command,
        detach_key,
        copy_mode_key,
        history_lines,
    })
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
    let key = spec
        .strip_prefix("Ctrl+")
        .ok_or_else(|| format!("unsupported key specification {spec:?}; expected Ctrl+…"))?;
    let byte = match key {
        "Space" => 0,
        "?" => 0x7f,
        _ => {
            let characters: Vec<_> = key.chars().collect();
            let character = *characters
                .first()
                .filter(|_| characters.len() == 1)
                .ok_or_else(|| format!("unsupported key specification {spec:?}"))?;
            if !character.is_ascii() {
                return Err(format!("unsupported key specification {spec:?}"));
            }
            character.to_ascii_uppercase() as u8 & 0x1f
        }
    };
    Ok(byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(contents: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("stay-config-{stamp}.toml"));
        fs::write(&path, contents).expect("write config fixture");
        path
    }

    fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn defaults_leave_the_command_unconfigured_and_use_key_defaults() {
        let config = load_from_path_and_env(None, &env(&[("SHELL", "/bin/zsh")])).unwrap();
        assert_eq!(config.default_command, None);
        assert_eq!(config.detach_key, 0x1c);
        assert_eq!(config.copy_mode_key, 0);
        assert_eq!(config.history_lines, 10_000);
    }

    #[test]
    fn defaults_leave_the_command_unconfigured_when_shell_is_unset() {
        let config = load_from_path_and_env(None, &BTreeMap::new()).unwrap();
        assert_eq!(config.default_command, None);
    }

    #[test]
    fn file_values_are_overridden_by_environment() {
        let path = fixture("default_command = \"fish\"\ndetach_key = \"Ctrl+A\"\ncopy_mode_key = \"Ctrl+B\"\nhistory_lines = 42\n");
        let config = load_from_path_and_env(
            Some(&path),
            &env(&[
                ("STAY_CMD", "nsh"),
                ("STAY_DETACH_KEY", "Ctrl+C"),
                ("STAY_COPY_MODE_KEY", "Ctrl+D"),
                ("STAY_HISTORY_LINES", "unlimited"),
            ]),
        )
        .unwrap();
        assert_eq!(config.default_command, Some("nsh".to_owned()));
        assert_eq!(config.detach_key, 3);
        assert_eq!(config.copy_mode_key, 4);
        assert_eq!(config.history_lines, UNLIMITED_HISTORY_LINES);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn accepts_string_history_and_rejects_invalid_values() {
        let path = fixture("history_lines = \"unlimited\"\n");
        assert_eq!(
            Config::load_from_path(&path).unwrap().history_lines,
            UNLIMITED_HISTORY_LINES
        );
        fs::remove_file(path).unwrap();
        let path = fixture("history_lines = 0\n");
        assert!(Config::load_from_path(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_key_collision_and_malformed_toml() {
        let path = fixture("detach_key = \"Ctrl+A\"\ncopy_mode_key = \"Ctrl+A\"\n");
        let error = Config::load_from_path(&path).unwrap_err();
        assert!(error.contains("detach_key") && error.contains("copy_mode_key"));
        fs::remove_file(path).unwrap();
        let path = fixture("history_lines = [");
        assert!(Config::load_from_path(&path)
            .unwrap_err()
            .contains("parse config file"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_control_key_specs() {
        assert_eq!(parse_key_spec("Ctrl+\\"), Ok(0x1c));
        assert_eq!(parse_key_spec("Ctrl+Space"), Ok(0));
        assert_eq!(parse_key_spec("Ctrl+?"), Ok(0x7f));
        assert!(parse_key_spec("Alt+A").is_err());
    }
}
