# Review: TASK-034

## Findings

### R001

Status: ADDRESSED

The generated instructions claim a POSIX-`sh` snippet that can be used from
`~/.zshrc`, and `src/prompt_integration.rs` explicitly describes zsh as a
target shell. However, the suggested prompt integration stores the literal
`$(stay_prompt_segment)` in `PS1`:

```sh
PS1='$(stay_prompt_segment)'"$PS1"
```

In zsh, command substitution in a prompt is only performed when the
`PROMPT_SUBST` option is enabled. The snippet neither enables that option nor
documents it, so a default zsh configuration displays the literal command
substitution instead of `[<name>] `. This fails the task's requirement that
the prompt reflect `STAY_SESSION_NAME` for the shell advertised by the
snippet's instructions. The official zsh documentation specifies this
requirement: [Prompt Expansion](https://zsh.sourceforge.io/Doc/Release/Prompt-Expansion.html).

Evidence of resolution: `src/prompt_integration.rs` now explicitly instructs
zsh users to enable `setopt PROMPT_SUBST`, and
`prompt_integration_snippet_reflects_the_session_name_in_a_zsh_prompt`
regresses both the literal-command-substitution behavior without the option
and the `[work]` result with it. CI installs zsh so the regression runs in
the hosted Linux gate as well.

## Final decision

Status: COMPLETED

R001 is addressed. The implementation is well-scoped and approved.

Independent verification: the exact `just qcheck` and `just mac-qcheck`
recipes passed. The first local attempt hit an unrelated existing
session-creation timing failure; the retry passed without source changes.
