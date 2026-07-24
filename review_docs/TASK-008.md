# Review: TASK-008

## Findings

### R001

Status: ADDRESSED

The PTY harness now uses a unique `stay-test-*` namespace through
[tests/attachment.rs](/home/nevd/stay/stay/tests/attachment.rs:36), and its
guard tears down that server with `kill-server`
([tests/attachment.rs](/home/nevd/stay/stay/tests/attachment.rs:48)). The
production namespace remains separately asserted as fixed to `stay`.

### R002

Status: ADDRESSED

The mandatory macOS verification gate is not passing, so this task cannot be
review-complete. The first `just mac-qcheck` attempt could not create Just's
temporary directory under the read-only `/run/user/1000`; retrying with
`XDG_RUNTIME_DIR=/tmp` reached `scripts/maccmd` but SSH failed with `Bad owner
or permissions` for the host system SSH configuration. `just qcheck` passed,
and `cargo test --locked --test attachment -- --nocapture` passed, but the
required macOS gate still needs a successful run after the implementation is
amended. The same SSH configuration failure reproduced on this review pass.
Using a local SSH wrapper with `-F /dev/null` bypassed that configuration
error, but the remote `cargo test --locked --all-targets --all-features`
remained silent for more than five minutes and had to be stopped; no passing
macOS gate was obtained. On the current commit, the same workaround again
remained silent and had to be stopped without producing a passing result.
The corrected temporary SSH config preserved the configured SSH user and
identity while bypassing only the malformed system config;
`just mac-qcheck` then passed on the current commit.

### R003

Status: ADDRESSED

The PTY test now launches the actual `stay` binary through `script`
([tests/attachment.rs](/home/nevd/stay/stay/tests/attachment.rs:147)). Its
test-local `tmux` shim remaps only the production `-L stay` calls to the
unique isolated namespace, while the binary still exercises normal CLI
dispatch and direct attach. The shim also preserves the production namespace
assertion separately.

## Final decision

Status: COMPLETED

Verification completed: `just qcheck` passed, and the focused attachment
tests passed. R001, R002, and R003 are addressed. The required
`just mac-qcheck` passed on the current commit using the corrected temporary
SSH configuration described in R002.
