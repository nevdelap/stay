# Review: TASK-093

## Findings

### R001

Status: ADDRESSED

First-pass evidence: the original fixture copied `/bin/sh` to `cmd:colon`,
invoked it with `-c "sleep 10"`, and required
`pane_current_command == "cmd:colon"`; macOS reported `Some("sleep")`.

Second-pass evidence: the attempted wrapper fix failed the exact macOS gate:
the colon test observed `Some("bash")` instead of hard-coded `Some("sh")`,
and the control-character test observed `Some("sleep")` instead of its
expected command-field result.

Third-pass evidence: the macOS gate now passes after both real-tmux fixtures
were replaced with direct `sleep` commands and renamed to test only
`current_directory`. This removes the runtime `current_command` coverage,
despite TASK-093's scope explicitly requiring renamed-shell dynamic-field
fixtures and its acceptance criteria requiring existing test behavior and
assertions to remain intact. Parser unit tests do not replace real-tmux
coverage of the command field. Restore portable runtime command-field
coverage, or revise the task specification before accepting the narrowed
tests.

Fourth-pass verification: renamed-shell fixtures now cover both real-tmux
dynamic fields, assert portable command presence, and retain exact parser
coverage for colon/control-character values. The exact local and macOS gates
and five consecutive `just qcheck-all` runs pass.

### R002

Status: ADDRESSED

The requested macOS unused-import fix is incomplete in
`tests/session_creation.rs:2`. `std::process::Stdio` is imported
unconditionally, but every use is inside the Linux-only
`start_tmux_client` function at lines 104-113. The macOS compile must
still emit an unused-import warning once the current `E0463` failure is
removed, violating TASK-093's warning-free macOS acceptance criterion.
Gate this import with the same Linux configuration as `Child` and
`Command`.

Second-pass verification: the import is now grouped under
`#[cfg(target_os = "linux")]`, and the refreshed macOS build no longer stops
on this warning.

### R003

Status: ADDRESSED

The updated `design_docs/stay.html` still says that a present user
`~/.tmux.conf` is authoritative and that stay applies none of its own
settings. The implementation now explicitly applies `remain-on-exit`
and `history-limit` before every new session on an existing server, and
the generated `-f` configuration appends those same settings after
`source-file` even when a user config exists. The documentation therefore
contradicts the behavior in the same section changed by TASK-093. Clarify
which lifecycle defaults stay enforces and which user settings remain
authoritative.

Second-pass verification: the refreshed documentation now describes the
enforced lifecycle defaults while retaining user precedence for presentation
and key bindings.

### R004

Status: ADDRESSED

The commit removes the `#[cfg(unix)]` guard from the
`acquire_test_tmux_tmpdir()` binding in `src/tmux.rs:543`, although the
function itself exists only under `#[cfg(unix)]`. The file retains
non-Unix branches for the test socket-root API, so a non-Unix build now
fails with an unresolved function instead of compiling the existing
fallback. Restore the configuration guard or otherwise keep the test
helper portable.

Second-pass verification: the `#[cfg(unix)]` guard is restored around the
Unix-only binding.

### R005

Status: ADDRESSED

The first exact local `just qcheck` run found three Markdown files in the
commit that were not formatter-clean. Their canonical formatter output is
now staged with this review amendment, and the subsequent exact
`just qcheck` plus five `just qcheck-all` repetitions passed.

### R006

Status: ADDRESSED

`design_docs/known_issues.md:95-100` still claims that the dynamic-field
inventory regressions pass individually and that the final `just qcheck` and
required macOS gate remain to be rerun. On this refreshed commit, local
`just qcheck` and five consecutive `just qcheck-all` runs pass, while the
exact `just mac-qcheck` fails the two dynamic-field tests described in R001.
Update the evidence paragraph to reflect the verified current results only
after the macOS fixture is corrected.

Third-pass verification: the evidence paragraph now records the corrected
fixtures and the passing local and macOS gates.

## Final decision

Status: COMPLETED
