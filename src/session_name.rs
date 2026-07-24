use std::fmt;

/// The reason a session name is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionNameError {
    Empty,
    DisallowedCharacter { character: char, position: usize },
}

impl SessionNameError {
    fn new(character: char, position: usize) -> Self {
        Self::DisallowedCharacter {
            character,
            position,
        }
    }
}

impl fmt::Display for SessionNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "invalid session name: must not be empty"),
            Self::DisallowedCharacter {
                character,
                position,
            } => write!(
                formatter,
                "invalid session name: disallowed character {} at position {}",
                format_character(*character),
                position
            ),
        }
    }
}

impl std::error::Error for SessionNameError {}

fn format_character(character: char) -> String {
    match character {
        '\n' => "\\n (newline)".to_owned(),
        '\r' => "\\r (carriage return)".to_owned(),
        '\t' => "\\t (tab)".to_owned(),
        '\u{1b}' => "ESC (0x1B)".to_owned(),
        '\u{7f}' => "DEL (0x7F)".to_owned(),
        character if character.is_ascii_control() => {
            format!("0x{:02X}", character as u32)
        }
        character => format!("'{}'", character.escape_default()),
    }
}

/// Validate a name before it is passed to tmux or rendered in the picker.
///
/// # Errors
///
/// Returns an error when the name is empty, contains tmux-disallowed
/// punctuation, or contains an ASCII control byte.
pub fn validate_session_name(name: &str) -> Result<(), SessionNameError> {
    if name.is_empty() {
        return Err(SessionNameError::Empty);
    }

    for (position, character) in name.chars().enumerate() {
        if matches!(character, '.' | ':') || character.is_ascii_control() {
            return Err(SessionNameError::new(character, position));
        }
    }

    Ok(())
}

/// Parse a session name for use as a clap value parser.
///
/// # Errors
///
/// Returns an error string when `validate_session_name` rejects the input.
pub fn parse_session_name(value: &str) -> Result<String, String> {
    validate_session_name(value)
        .map(|()| value.to_owned())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_session_name;

    #[test]
    fn ordinary_names_are_valid() {
        assert!(validate_session_name("work-1_東京").is_ok());
    }

    #[test]
    fn empty_names_are_rejected() {
        let error = validate_session_name("").unwrap_err().to_string();
        assert_eq!(error, "invalid session name: must not be empty");
    }

    #[test]
    fn tmux_disallowed_characters_are_rejected() {
        for character in ['.', ':', '\n'] {
            let error = validate_session_name(&format!("ok{character}name")).unwrap_err();
            assert!(error.to_string().contains("position 2"));
        }
    }

    #[test]
    fn control_and_escape_bytes_are_rejected() {
        for character in ['\x01', '\x1B', '\x7F', '\r', '\t'] {
            let error = validate_session_name(&format!("ok{character}name")).unwrap_err();
            assert!(error.to_string().contains("position 2"));
        }
    }

    #[test]
    fn disallowed_characters_at_boundaries_are_rejected() {
        for name in [".name", "name:", "\x01name", "name\x7F"] {
            let error = validate_session_name(name).unwrap_err();
            assert!(
                error.to_string().contains("position 0")
                    || error.to_string().contains("position 4")
            );
        }
    }

    #[test]
    fn errors_identify_character_and_position() {
        let error = validate_session_name("abc\x1Bdef").unwrap_err().to_string();
        assert!(error.contains("ESC"));
        assert!(error.contains("position 3"));
    }
}
