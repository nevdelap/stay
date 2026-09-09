use crate::session_name::parse_session_name;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDefinition {
    pub name: String,
    pub created: u64,
    pub cwd: String,
    pub command: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    sessions: Vec<SessionDefinition>,
}

#[derive(Debug)]
pub enum StoreError {
    Read(String),
    Invalid(String),
    Write { message: String, committed: bool },
}

impl StoreError {
    #[must_use]
    pub fn committed(&self) -> bool {
        matches!(
            self,
            Self::Write {
                committed: true,
                ..
            }
        )
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(message) | Self::Invalid(message) => formatter.write_str(message),
            Self::Write { message, committed } => {
                if *committed {
                    write!(
                        formatter,
                        "{message}; the new session store is in place but directory durability is uncertain"
                    )
                } else {
                    formatter.write_str(message)
                }
            }
        }
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitStatus {
    Durable,
    Uncertain(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionStore {
    path: PathBuf,
    #[cfg(test)]
    force_uncertain_after_rename: bool,
    #[cfg(test)]
    force_failure_before_rename: bool,
}

impl SessionStore {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            #[cfg(test)]
            force_uncertain_after_rename: false,
            #[cfg(test)]
            force_failure_before_rename: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_uncertain_parent_sync(mut self) -> Self {
        self.force_uncertain_after_rename = true;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_failure_before_rename(mut self) -> Self {
        self.force_failure_before_rename = true;
        self
    }

    /// Returns the platform-specific path for Stay's session store.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not expose a user
    /// configuration directory.
    pub fn default_path() -> Result<PathBuf, String> {
        dirs::config_dir()
            .map(|directory| directory.join("stay/sessions.toml"))
            .ok_or_else(|| "could not determine the user configuration directory".to_owned())
    }

    /// Opens the session store at the platform-specific default path.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not expose a user
    /// configuration directory.
    pub fn open_default() -> Result<Self, String> {
        Self::default_path().map(Self::from_path)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads and validates the complete store snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be read, parsed, or validated.
    pub fn load(&self) -> Result<BTreeMap<String, SessionDefinition>, StoreError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => {
                return Err(StoreError::Read(format!(
                    "failed to read session store {} ({error})",
                    self.path.display()
                )));
            }
        };
        let file = toml::from_str::<StoreFile>(&contents).map_err(|error| {
            StoreError::Invalid(format!(
                "failed to parse session store {} ({error})",
                self.path.display()
            ))
        })?;
        if file.version != STORE_VERSION {
            return Err(StoreError::Invalid(format!(
                "unsupported session store version {} in {} (expected {})",
                file.version,
                self.path.display(),
                STORE_VERSION
            )));
        }
        let mut sessions = BTreeMap::new();
        for definition in file.sessions {
            validate_definition(&definition)?;
            if sessions
                .insert(definition.name.clone(), definition.clone())
                .is_some()
            {
                return Err(StoreError::Invalid(format!(
                    "session store {} contains duplicate session {:?}",
                    self.path.display(),
                    definition.name
                )));
            }
        }
        Ok(sessions)
    }

    /// Commits a complete store snapshot using an atomic, durable replacement.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or any filesystem step fails. An
    /// error after rename reports that the new snapshot is committed but its
    /// directory durability is uncertain.
    #[allow(clippy::too_many_lines)]
    pub fn commit(
        &self,
        sessions: &BTreeMap<String, SessionDefinition>,
    ) -> Result<CommitStatus, StoreError> {
        for (name, definition) in sessions {
            if name != &definition.name {
                return Err(StoreError::Invalid(format!(
                    "session store key {:?} does not match session name {:?}",
                    name, definition.name
                )));
            }
            validate_definition(definition)?;
        }

        let parent = self.path.parent().ok_or_else(|| StoreError::Write {
            message: format!(
                "session store {} has no parent directory",
                self.path.display()
            ),
            committed: false,
        })?;
        fs::create_dir_all(parent).map_err(|error| StoreError::Write {
            message: format!(
                "failed to create session store directory {} ({error})",
                parent.display()
            ),
            committed: false,
        })?;
        set_private_directory(parent)?;

        let temporary = temporary_path(&self.path);
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| StoreError::Write {
                    message: format!(
                        "failed to create temporary session store {} ({error})",
                        temporary.display()
                    ),
                    committed: false,
                })?;
            let file_contents = StoreFile {
                version: STORE_VERSION,
                sessions: sessions.values().cloned().collect(),
            };
            let serialized =
                toml::to_string_pretty(&file_contents).map_err(|error| StoreError::Write {
                    message: format!("failed to serialize session store: {error}"),
                    committed: false,
                })?;
            file.write_all(serialized.as_bytes())
                .map_err(|error| StoreError::Write {
                    message: format!(
                        "failed to write temporary session store {} ({error})",
                        temporary.display()
                    ),
                    committed: false,
                })?;
            file.write_all(b"\n").map_err(|error| StoreError::Write {
                message: format!(
                    "failed to finish temporary session store {} ({error})",
                    temporary.display()
                ),
                committed: false,
            })?;
            file.sync_all().map_err(|error| StoreError::Write {
                message: format!(
                    "failed to sync temporary session store {} ({error})",
                    temporary.display()
                ),
                committed: false,
            })?;
            drop(file);
            #[cfg(test)]
            if self.force_failure_before_rename {
                return Err(StoreError::Write {
                    message: "injected pre-rename store failure".to_owned(),
                    committed: false,
                });
            }
            fs::rename(&temporary, &self.path).map_err(|error| StoreError::Write {
                message: format!(
                    "failed to replace session store {} ({error})",
                    self.path.display()
                ),
                committed: false,
            })?;
            #[cfg(test)]
            if self.force_uncertain_after_rename {
                return Ok(CommitStatus::Uncertain(
                    "injected parent-directory sync failure".to_owned(),
                ));
            }
            let directory = match File::open(parent) {
                Ok(directory) => directory,
                Err(error) => {
                    return Ok(CommitStatus::Uncertain(format!(
                        "failed to open session store directory {} ({error})",
                        parent.display()
                    )));
                }
            };
            if let Err(error) = directory.sync_all() {
                return Ok(CommitStatus::Uncertain(format!(
                    "failed to sync session store directory {} ({error})",
                    parent.display()
                )));
            }
            Ok(CommitStatus::Durable)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn validate_definition(definition: &SessionDefinition) -> Result<(), StoreError> {
    parse_session_name(&definition.name).map_err(|error| {
        StoreError::Invalid(format!(
            "invalid saved session name {:?}: {error}",
            definition.name
        ))
    })?;
    if definition.cwd.is_empty() {
        return Err(StoreError::Invalid(format!(
            "saved session {:?} has an empty working directory",
            definition.name
        )));
    }
    if definition.command.is_empty() || definition.command[0].is_empty() {
        return Err(StoreError::Invalid(format!(
            "saved session {:?} has an empty command",
            definition.name
        )));
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".tmp-{}-{}", std::process::id(), unique_suffix()));
    PathBuf::from(temporary)
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn set_private_directory(path: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            StoreError::Write {
                message: format!(
                    "failed to secure session store directory {} ({error})",
                    path.display()
                ),
                committed: false,
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CommitStatus, SessionDefinition, SessionStore, StoreError};
    use std::collections::BTreeMap;
    use std::fs;

    fn definition(name: &str) -> SessionDefinition {
        SessionDefinition {
            name: name.to_owned(),
            created: 42,
            cwd: "/tmp".to_owned(),
            command: vec!["/bin/sh".to_owned()],
        }
    }

    #[test]
    fn commit_round_trips_sorted_definitions() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("stay/sessions.toml"));
        let mut sessions = BTreeMap::new();
        sessions.insert("zeta".to_owned(), definition("zeta"));
        sessions.insert("alpha".to_owned(), definition("alpha"));
        store.commit(&sessions).expect("commit store");
        assert_eq!(store.load().expect("load store"), sessions);
        let contents = fs::read_to_string(store.path()).expect("read store");
        assert!(contents.contains("version = 1"));
        assert!(
            contents.find("name = \"alpha\"").unwrap() < contents.find("name = \"zeta\"").unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_store_is_empty() {
        let store = SessionStore::from_path(tempfile_path().join("missing.toml"));
        assert!(store.load().expect("load missing store").is_empty());
    }

    #[test]
    fn rejects_unknown_version_and_duplicate_names() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        fs::create_dir_all(&root).expect("create root");
        fs::write(store.path(), "version = 2\n").expect("write version");
        assert!(matches!(store.load(), Err(StoreError::Invalid(_))));
        fs::write(
            store.path(),
            "version = 1\n[[sessions]]\nname = \"same\"\ncreated = 1\ncwd = \"/tmp\"\ncommand = [\"sh\"]\n[[sessions]]\nname = \"same\"\ncreated = 2\ncwd = \"/tmp\"\ncommand = [\"sh\"]\n",
        )
        .expect("write duplicate store");
        assert!(matches!(store.load(), Err(StoreError::Invalid(_))));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_missing_and_extra_schema_fields_without_partial_loading() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        fs::create_dir_all(&root).expect("create root");
        fs::write(
            store.path(),
            "version = 1\n[[sessions]]\nname = \"one\"\ncreated = 1\ncwd = \"/tmp\"\n",
        )
        .expect("write missing-field store");
        assert!(matches!(store.load(), Err(StoreError::Invalid(_))));
        fs::write(
            store.path(),
            "version = 1\n[[sessions]]\nname = \"one\"\ncreated = 1\ncwd = \"/tmp\"\ncommand = [\"sh\"]\nextra = true\n",
        )
        .expect("write extra-field store");
        assert!(matches!(store.load(), Err(StoreError::Invalid(_))));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_uncertain_commit_after_the_store_has_been_replaced() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        let mut sessions = BTreeMap::new();
        sessions.insert("one".to_owned(), definition("one"));
        let status = store
            .clone()
            .with_uncertain_parent_sync()
            .commit(&sessions)
            .expect("injected post-rename failure is a committed result");
        assert_eq!(
            status,
            CommitStatus::Uncertain("injected parent-directory sync failure".to_owned())
        );
        assert_eq!(store.load().expect("load replaced store"), sessions);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preserves_the_previous_store_when_commit_fails_before_rename() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        let mut previous = BTreeMap::new();
        previous.insert("old".to_owned(), definition("old"));
        store.commit(&previous).expect("write previous store");

        let mut desired = BTreeMap::new();
        desired.insert("new".to_owned(), definition("new"));
        let error = store
            .clone()
            .with_failure_before_rename()
            .commit(&desired)
            .expect_err("injected pre-rename failure");
        assert!(!error.committed());
        assert!(error.to_string().contains("injected pre-rename"));
        assert_eq!(store.load().expect("load previous store"), previous);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unwritable_store_parent_without_replacing_existing_data() {
        let root = tempfile_path();
        fs::create_dir_all(&root).expect("create root");
        let parent_file = root.join("not-a-directory");
        fs::write(&parent_file, "existing").expect("write parent blocker");
        let store = SessionStore::from_path(parent_file.join("sessions.toml"));
        let mut sessions = BTreeMap::new();
        sessions.insert("blocked".to_owned(), definition("blocked"));
        let error = store.commit(&sessions).expect_err("unwritable parent");
        assert!(!error.committed());
        assert!(
            error
                .to_string()
                .contains("failed to create session store directory")
        );
        assert_eq!(
            fs::read_to_string(parent_file).expect("read blocker"),
            "existing"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn round_trips_special_command_arguments_including_empty_arguments() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        let definition = SessionDefinition {
            name: "special_東京-name".to_owned(),
            created: 7,
            cwd: "/tmp/with spaces/\"quotes\\slashes$".to_owned(),
            command: vec![
                "/bin/echo".to_owned(),
                String::new(),
                "two words \"quoted\" \\$ 東京".to_owned(),
            ],
        };
        let sessions = [(definition.name.clone(), definition)]
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        store.commit(&sessions).expect("commit special arguments");
        assert_eq!(store.load().expect("load special arguments"), sessions);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn round_trips_the_complete_supported_name_and_value_matrix() {
        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        let mut sessions = BTreeMap::new();
        for (name, cwd) in [
            ("hyphen-name", "/tmp/with spaces/\"quotes\\slashes$"),
            ("under_score", "/tmp/東京/with spaces"),
            ("東京", "/tmp/empty-argument"),
        ] {
            sessions.insert(
                name.to_owned(),
                SessionDefinition {
                    name: name.to_owned(),
                    created: 7,
                    cwd: cwd.to_owned(),
                    command: vec![
                        "/bin/echo".to_owned(),
                        String::new(),
                        "two words \"quoted\" \\$ 東京".to_owned(),
                    ],
                },
            );
        }
        store.commit(&sessions).expect("commit supported matrix");
        assert_eq!(store.load().expect("load supported matrix"), sessions);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_the_complete_invalid_name_matrix_and_empty_working_directory() {
        for character in [
            '.', ':', '\n', '\r', '\t', '\u{1f}', '\u{2028}', '\u{2029}', '\u{202e}', '\u{2066}',
        ] {
            let root = tempfile_path();
            let store = SessionStore::from_path(root.join("sessions.toml"));
            let name = format!("invalid{character}name");
            let mut sessions = BTreeMap::new();
            sessions.insert(name.clone(), definition(&name));
            assert!(matches!(
                store.commit(&sessions),
                Err(StoreError::Invalid(_))
            ));
            let _ = fs::remove_dir_all(root);
        }

        let root = tempfile_path();
        let store = SessionStore::from_path(root.join("sessions.toml"));
        let mut sessions = BTreeMap::new();
        let mut invalid = definition("missing-cwd");
        invalid.cwd.clear();
        sessions.insert(invalid.name.clone(), invalid);
        assert!(matches!(
            store.commit(&sessions),
            Err(StoreError::Invalid(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    fn tempfile_path() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "stay-store-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
