//! The `--prompt-integration` shell snippet, printed to stdout so it can be
//! sourced or `eval`'d from a shell rc file (same convention as `starship
//! init`/`direnv hook`).

/// Returns the `--prompt-integration` snippet.
///
/// Valid POSIX `sh` — and therefore also bash and zsh, both POSIX-superset
/// shells — so it can be sourced or `eval`'d directly. The leading `#`
/// comments are the usage instructions; they're inert once sourced. zsh
/// additionally needs `setopt PROMPT_SUBST` for `PS1` to expand a command
/// substitution at all (bash does this unconditionally), so the
/// instructions call that out explicitly for zsh users.
#[must_use]
pub fn snippet() -> &'static str {
    "\
# stay prompt integration.
# Add this to your shell rc file (~/.bashrc, ~/.zshrc, ~/.profile, etc.):
#   eval \"$(stay --prompt-integration)\"
# then reference stay_prompt_segment from your own prompt, e.g.:
#   PS1='$(stay_prompt_segment)'\"$PS1\"
# zsh only expands command substitutions in PS1 when PROMPT_SUBST is set,
# so zsh users must also add this (bash expands them by default):
#   setopt PROMPT_SUBST
# Prints \"[<name>] \" when run inside a stay-created session's pane
# (STAY_SESSION_NAME set and non-empty); prints nothing otherwise.
stay_prompt_segment() {
    if [ -n \"${STAY_SESSION_NAME:-}\" ]; then
        printf '[%s] ' \"$STAY_SESSION_NAME\"
    fi
}
"
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn snippet_is_non_empty_and_defines_the_segment_function() {
        let text = snippet();
        assert!(!text.is_empty());
        assert!(text.contains("stay_prompt_segment"));
        assert!(text.contains("STAY_SESSION_NAME"));
        assert!(text.contains("setopt PROMPT_SUBST"));
    }
}
