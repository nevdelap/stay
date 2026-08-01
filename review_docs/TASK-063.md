# Review: TASK-063

## Findings

### R001

Status: ADDRESSED

The new shared `TMUX_TMPDIR` cleanup removes the socket directory but does not
terminate the tmux servers that the attachment tests leave behind. The
attachment `SessionGuard` owns a `Tmux` wrapper but has no drop cleanup, and
`TestTmuxTmpDir::drop` only calls `remove_dir_all`. A direct probe confirms that
the tmux server remains alive after its socket root is removed, so a passing
suite can leak live tmux servers and their pane processes. This violates the
task's self-cleaning acceptance criterion. Cleanup must terminate the test
servers before removing their socket root, while remaining scoped to the test
namespaces. The updated cleanup enumerates the owned socket root and issues
`kill-server` for each test namespace before removing the directory; the full
suite then leaves no matching tmux processes or owned socket-root directories.

## Verification

- Reviewed the complete `TASK-063` diff against `3b93acc`.
- Verified that removing a tmux socket root leaves the server process alive.
- The updated server cleanup addresses R001; the full `just qcheck` passed.
- The exact `just mac-qcheck` recipe passed.
- The package version advances from `0.0.44` to `0.0.45`.

## Final decision

Status: COMPLETED

R001 is addressed and TASK-063 is approved.
