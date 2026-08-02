# stay release checklist

This checklist covers the private preparation for `stay` version `0.0.49`. It
also records the handoff to TASK-068, which is the only task allowed to publish
the crate, create or push a release tag, or configure external release services.

## Resolved release

- Package: `stay`

- Version: `0.0.49`

- Repository: `https://github.com/nevdelap/stay`

- Release commit: capture the exact commit after the private preparation is
  complete and present on `origin/main`:

  ```sh
  release_commit=$(git rev-parse HEAD)
  printf '%s\n' "$release_commit"
  git show --no-patch --format=fuller "$release_commit"
  ```

  Record that SHA in the release notes. Do not rely on a moving branch name.

## TASK-037 private preparation

Run these checks from a clean checkout. None of them publishes to crates.io,
creates a tag, pushes a tag, or configures credentials:

1. Confirm the checkout, package version, and worktree:

   ```sh
   git status --short
   cargo metadata --format-version 1 --no-deps
   ```

   The worktree must be empty and the package version must be `0.0.49`.

2. Inspect the files that would be included in the package:

   ```sh
   cargo package --locked --list
   ```

3. Run the private repository and release checks:

   ```sh
   just qcheck
   just mac-qcheck
   cargo publish --locked --dry-run
   ```

   The publish command is a dry run only. Do not follow it with
   `cargo publish --locked`.

4. Verify installation from this checkout without using crates.io:

   ```sh
   install_root=$(mktemp -d)
   trap 'rm -rf -- "$install_root"' EXIT
   CARGO_INSTALL_ROOT="$install_root" cargo install --locked --path .
   test "$("$install_root/bin/stay" --version)" = 'stay 0.0.49'
   ```

5. Capture the private preparation commit and retain the clean-worktree and CI
   results with it. The exact release commit must be checked against
   `origin/main` before any public action in TASK-068.

## TASK-068 public handoff

The following actions are deferred until TASK-068 and must not be performed as
part of private preparation:

- checking or registering the `stay` name on crates.io;
- using a crates.io account, token, credential, or trusted publisher;
- running the real `cargo publish --locked` command;
- creating or pushing the annotated `v0.0.49` release tag; and
- enabling GitHub release automation or changing repository settings.

TASK-068 will re-resolve the version from the captured release commit, perform
the final registry ownership check, publish once, verify the registry install,
configure Trusted Publishing for the guarded workflow, and then hand the
immutable tag to that workflow. Until then, keep the package and all release
verification private.
