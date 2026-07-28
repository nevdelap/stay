//! The `shell-integration` subcommand and its optional short alias.

use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::PathBuf,
};

use crate::prompt_integration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RcFile {
    label: &'static str,
    path: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
struct Output {
    stdout: String,
    warning: Option<String>,
}

/// Print the shell integration snippet and, when requested, a safe `s` alias.
///
/// # Errors
///
/// Returns an error when a configured shell rc file cannot be read or when
/// writing the output fails.
pub fn run(s_alias: bool) -> Result<(), String> {
    let output = if s_alias {
        let rc_files = production_rc_files();
        render(
            prompt_integration::snippet(),
            true,
            std::env::var_os("PATH").as_deref(),
            &rc_files,
        )?
    } else {
        render(prompt_integration::snippet(), false, None, &[])?
    };

    io::stdout()
        .write_all(output.stdout.as_bytes())
        .map_err(|error| format!("failed to write stdout: {error}"))?;
    if let Some(warning) = output.warning {
        writeln!(io::stderr(), "{warning}")
            .map_err(|error| format!("failed to write stderr: {error}"))?;
    }
    Ok(())
}

fn production_rc_files() -> Vec<RcFile> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    [
        ("alias in ~/.bashrc", ".bashrc"),
        ("alias in ~/.zshrc", ".zshrc"),
        ("alias in ~/.profile", ".profile"),
    ]
    .into_iter()
    .map(|(label, name)| RcFile {
        label,
        path: home.join(name),
    })
    .collect()
}

fn render(
    snippet: &str,
    s_alias: bool,
    path_var: Option<&OsStr>,
    rc_files: &[RcFile],
) -> Result<Output, String> {
    let mut stdout = snippet.to_owned();
    if !s_alias {
        return Ok(Output {
            stdout,
            warning: None,
        });
    }

    if let Some(label) = find_rc_conflict(rc_files)? {
        return Ok(Output {
            stdout,
            warning: Some(conflict_warning(label)),
        });
    }
    if find_path_conflict(path_var) {
        return Ok(Output {
            stdout,
            warning: Some(conflict_warning("command on PATH")),
        });
    }

    stdout.push_str("alias s=stay\n");
    Ok(Output {
        stdout,
        warning: None,
    })
}

fn find_rc_conflict(rc_files: &[RcFile]) -> Result<Option<&'static str>, String> {
    for rc_file in rc_files {
        let Ok(contents) = fs::read_to_string(&rc_file.path) else {
            if rc_file.path.exists() {
                return Err(format!(
                    "failed to read shell rc file {}",
                    rc_file.path.display()
                ));
            }
            continue;
        };
        if contents
            .lines()
            .any(|line| line.trim_start().starts_with("alias s="))
        {
            return Ok(Some(rc_file.label));
        }
    }
    Ok(None)
}

fn find_path_conflict(path_var: Option<&OsStr>) -> bool {
    path_var.is_some_and(|path| {
        std::env::split_paths(path).any(|directory| directory.join("s").exists())
    })
}

fn conflict_warning(source: &str) -> String {
    format!(
        "warning: an 's' {source} already exists; skipping 'alias s=stay' — add it yourself if you want to override it"
    )
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{render, RcFile};
    use crate::prompt_integration;

    fn fixture_path(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("stay-shell-integration-{label}-{timestamp}"))
    }

    fn rc_file(label: &'static str, path: &Path) -> RcFile {
        RcFile {
            label,
            path: path.to_owned(),
        }
    }

    #[test]
    fn without_alias_never_checks_conflict_sources() {
        let output = render(
            prompt_integration::snippet(),
            false,
            Some(OsStr::new("/path/that/does/not/exist")),
            &[rc_file(
                "alias in ~/.bashrc",
                Path::new("/path/that/does/not/exist"),
            )],
        )
        .expect("render without alias");

        assert_eq!(output.stdout, prompt_integration::snippet());
        assert_eq!(output.warning, None);
    }

    #[test]
    fn clean_sources_append_alias() {
        let path = OsString::from(fixture_path("clean-path"));
        let output = render(
            prompt_integration::snippet(),
            true,
            Some(path.as_os_str()),
            &[],
        )
        .expect("render clean alias");

        assert_eq!(
            output.stdout,
            format!("{}alias s=stay\n", prompt_integration::snippet())
        );
        assert_eq!(output.warning, None);
    }

    #[test]
    fn path_conflict_omits_alias_and_warns() {
        let directory = fixture_path("path-conflict");
        fs::create_dir(&directory).expect("create PATH fixture");
        fs::write(directory.join("s"), "").expect("create s fixture");
        let path = OsString::from(&directory);

        let output = render(prompt_integration::snippet(), true, Some(&path), &[])
            .expect("render PATH conflict");

        assert_eq!(output.stdout, prompt_integration::snippet());
        assert_eq!(
            output.warning.as_deref(),
            Some("warning: an 's' command on PATH already exists; skipping 'alias s=stay' — add it yourself if you want to override it")
        );
        fs::remove_dir_all(directory).expect("remove PATH fixture");
    }

    #[test]
    fn rc_conflict_omits_alias_and_warns() {
        let path = fixture_path("bashrc-conflict");
        fs::write(&path, "  alias s='stay'\n").expect("create rc fixture");
        let path_var = OsString::from(fixture_path("clean-path"));

        let output = render(
            prompt_integration::snippet(),
            true,
            Some(path_var.as_os_str()),
            &[rc_file("alias in ~/.bashrc", &path)],
        )
        .expect("render rc conflict");

        assert_eq!(output.stdout, prompt_integration::snippet());
        assert_eq!(
            output.warning.as_deref(),
            Some("warning: an 's' alias in ~/.bashrc already exists; skipping 'alias s=stay' — add it yourself if you want to override it")
        );
        fs::remove_file(path).expect("remove rc fixture");
    }

    #[test]
    fn differently_cased_sources_do_not_conflict() {
        let path = fixture_path("uppercase");
        fs::write(&path, "alias S=stay\n").expect("create uppercase rc fixture");
        let path_var = OsString::from(fixture_path("uppercase-path"));

        let output = render(
            prompt_integration::snippet(),
            true,
            Some(path_var.as_os_str()),
            &[rc_file("alias in ~/.bashrc", &path)],
        )
        .expect("render uppercase sources");

        assert_eq!(
            output.stdout,
            format!("{}alias s=stay\n", prompt_integration::snippet())
        );
        assert_eq!(output.warning, None);
        fs::remove_file(path).expect("remove uppercase rc fixture");
    }
}
