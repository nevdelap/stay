# stay release checklist

This checklist covers the private preparation for `stay` version `0.0.49`. It
also records the handoff to TASK-068, which is the only task allowed to publish
the crate, create or push a release tag, or configure external release services.

## Resolved release

- Package: `stay`

- Version: `0.0.49`

- Repository: `https://github.com/nevdelap/stay`

- TASK-037 private-preparation commit: retain it for the private metadata and
  package verification history, but do not use it for the first release tag.

- Final release commit: capture this only after TASK-067 is complete and its
  workflow is present on `origin/main`:

  ```sh
  set -euo pipefail
  git fetch origin main --no-tags
  release_commit=$(git rev-parse HEAD)
  version="$(
      sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -n 1
  )"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
  git merge-base --is-ancestor "$release_commit" origin/main
  git cat-file -e "$release_commit:.github/workflows/release.yml"
  printf '%s\n' "$release_commit"
  git show --no-patch --format=fuller "$release_commit"
  ```

  Record that SHA in the release notes and use it for the annotated tag. Do not
  reuse the TASK-037 SHA or rely on a moving branch name.

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
   results with it. This is a historical preparation SHA, not the first release
   tag target.

## TASK-067 dormant automation

The tagged-release workflow in `.github/workflows/release.yml` is committed but
dormant. It runs only for pushed stable version tags and fails before any
registry query, OIDC request, or publish step unless the repository variable
`RELEASE_AUTOMATION_ENABLED` is exactly `true`. Leave that variable unset or
disabled during TASK-037 and TASK-067.

The workflow checks the tag, package version, main-line ancestry, and the full
successful `CI` run (`check`, `msrv`, and `macos`) for the tagged commit before
it checks crates.io. It then runs the Linux quality gate and locked publish dry
run. A missing or unsuccessful check stops the release before Trusted Publishing
authentication.

An HTTP 404 for the exact crates.io version selects publish mode. An HTTP 200
selects verification-only mode and never republishes that version. Other HTTP
responses stop the workflow. Successful publication is polled for at most 60
attempts at 10-second intervals, and registry installation retries at most 12
times at 10-second intervals only for index or registry availability errors.
Build, package, and version errors stop immediately. The installed binary must
report exactly `stay <version>`.

Before the first tag is created, capture the final release commit using the
Resolved release commands above, after TASK-067 is complete and that commit is
present on `origin/main`. The `git cat-file` check must succeed; it prevents a
TASK-037-only SHA from starting a tag without the dormant workflow.

## TASK-068 public handoff

The following actions are deferred until TASK-068 and must not be performed as
part of private preparation:

- checking or registering the `stay` name on crates.io;
- using a crates.io account, token, credential, or trusted publisher;
- running the real `cargo publish --locked` command;
- creating or pushing the annotated `v0.0.49` release tag; and
- enabling GitHub release automation or changing repository settings.

TASK-068 is the one-time public bootstrap. It will re-resolve the version from
the captured release commit, perform the final registry ownership check, publish
once, verify the registry install, configure Trusted Publishing for the guarded
workflow, and then hand the immutable tag to that workflow.

### One-time Trusted Publishing setup

After the manual publication and registry install verification succeed, a
maintainer with GitHub administration rights must configure the crates.io
Trusted Publisher for exactly:

- repository: `nevdelap/stay`;
- workflow: `.github/workflows/release.yml`; and
- GitHub environment: `release`.

Configure any required protection or approval rules on the `release`
environment. Then set the repository variable `RELEASE_AUTOMATION_ENABLED=true`.
Do this only after the first manual publication succeeds and the tagged workflow
is ready for verification. Do not create a long-lived crates.io token, use
`cargo login` in CI, or store a registry credential in GitHub secrets.

### First manual bootstrap

The first release remains manual. From the exact clean release commit, the
maintainer should:

1. Confirm the package version, clean worktree, green CI, and private checks.
   Run the Resolved release commands now, after TASK-067 is on `origin/main`, so
   they define and verify both `release_commit` and `version`.

2. Check the crates.io package endpoint and require HTTP 404 immediately before
   the one-time `just publish` action.

3. Run `just publish` once and record whether crates.io accepted the upload.

4. Poll the exact version endpoint, then install the published version into a
   fresh temporary `CARGO_INSTALL_ROOT` and verify `stay --version`.

5. Configure Trusted Publishing and enable the repository variable as described
   above.

6. Create an annotated tag pointing to that final workflow-bearing SHA and push
   it without force:

   ```sh
   set -euo pipefail
   : "${release_commit:?run the Resolved release commands first}"
   version="$(
       sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -n 1
   )"
   [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
   git tag -a "v$version" "$release_commit" -m "Release $version"
   test "$(git rev-parse "v$version^{}")" = "$release_commit"
   git push origin "v$version"
   ```

   The tag push starts the workflow. Because the first version is already
   published, the workflow must take verification-only mode and must not publish
   it again.

### Later automated releases

Later releases require a new patch version in `Cargo.toml` and `Cargo.lock`, a
matching stable `v<version>` tag on a commit reachable from `main`, and a
successful full CI run. Keep the variable enabled only after the external
Trusted Publisher and environment are correctly configured. A normal release
then consists of merging the version bump to `main`, waiting for CI, creating
the annotated matching tag, and pushing that tag without force. The workflow
performs the remaining checks, publishes exactly once for an unpublished
version, and verifies the registry installation.

## Recovery

If publication succeeded but polling, installation, tag creation, tag push, or
workflow activation fails, do not republish, yank, replace, or force-push the
tag. Inspect the crates.io API, the exact version state, and the GitHub Actions
logs. Fix configuration errors, retry only safe verification or a non-force tag
push when the tag does not already exist, and use the workflow's
already-published verification-only mode. Stop and ask a maintainer when the
registry state, tag target, account authority, or Trusted Publishing
configuration is uncertain; do not guess or race the first registration.
