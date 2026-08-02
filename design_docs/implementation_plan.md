# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

Completed task entries are removed from this active plan; their history is
preserved in git (the task commit and its `Reviewed:` section). Add new work as
the next stable task entry; do not reuse an identifier from a removed task.

## TASK-037 - prepare the private crates.io release for `stay`

State: COMPLETED

Goal:

- Prepare all release metadata and private verification for the first `stay`
  release without publishing, tagging, pushing a release tag, or changing
  anything on crates.io. Leave the actual public bootstrap to TASK-068 after
  TASK-067's automation is complete.

Release decisions and current baseline:

- The package name is `stay`; availability and ownership are checked only by
  TASK-068 immediately before the first public publication.
- The release license is MIT, with the exact copyright line specified below.
- The current package version is `0.0.48`; the intended release is `0.0.49`. If
  another patch release commit lands before TASK-037 is unblocked, use the next
  patch version from that new baseline instead, and use one resolved release
  version consistently in every file and command.
- The repository URL is `https://github.com/nevdelap/stay`.
- No step in TASK-037 may upload to crates.io, create or push a release tag,
  configure an external trusted publisher, or otherwise expose the release to
  the wider world. The task implementation and tests must remain private.

Dependencies:

- The historical Issue 1 follow-up tasks TASK-039 through TASK-048 must be
  `COMPLETED` in Git history.
- The historical project-review and automation tasks TASK-054 through TASK-066
  must be `COMPLETED` in Git history. The release must include the cleaned
  public API, README and manifest-quality gates, final test/CI fixes, and picker
  safety fixes from those tasks.
- Before implementation starts, no other task may be `NEW` or
  `REVIEWED_FOUND_ISSUES`; all release-blocking review work must be complete.
- No operator credentials or external service configuration are required for
  TASK-037. Any future credentials belong only to the operator-facing steps in
  TASK-068 and must never be committed, printed, or added to documentation.

Scope:

- `Cargo.toml`: set the following exact publish metadata:
  - `description = "A terminal session manager for persistent tmux sessions."`
  - `license = "MIT"`
  - `repository = "https://github.com/nevdelap/stay"`
  - `readme = "README.md"`
  - `keywords = ["tmux", "terminal", "session-manager"]`
  - `categories = ["command-line-utilities"]`
- Add a new root `LICENSE` containing the standard MIT license text with
  `Copyright (c) 2026 Nev Delap`.
- Bump only the patch component of the package version exactly once from the
  task baseline; for the current baseline this is `0.0.48` → `0.0.49`. Update
  the matching package entry in `Cargo.lock`. Keep `tests/cli_help.rs` as a
  dynamic assertion that `stay --version` equals `env!("CARGO_PKG_VERSION")`; do
  not add a second hard-coded version assertion.
- Add `docs/release.md` as a tracked, operator-facing checklist for the private
  preparation phase and the later TASK-068 handoff. It must identify the
  resolved release version, the exact release commit to capture, private
  clean-worktree and CI checks, the local package dry run, and clearly mark all
  registry, credential, tag, and push steps as deferred until TASK-068.
- Add a short link in `README.md` to `docs/release.md`, and document both
  `cargo install stay` for a published release and `cargo install --path .` for
  a checkout build.
- Run only private verification in this task: `just qcheck`, `just mac-qcheck`,
  `cargo package --locked --list`, and `cargo publish --locked --dry-run`. The
  dry run must not be followed by `cargo publish --locked`.
- Verify the local source install without crates.io by creating a fresh
  temporary install root, running
  `CARGO_INSTALL_ROOT=<install-root> cargo install --locked --path .`, and
  checking that the installed binary reports `stay <release-version>`. Remove
  the temporary root afterward.
- Do not add `just publish` in this task. TASK-068 adds the real manual publish
  recipe, registry ownership check, publication verification, and immutable tag
  handoff after TASK-067 is complete.

Acceptance criteria:

- All dependency tasks listed above are `COMPLETED`, and no other release
  blocker remains in the implementation plan.
- `Cargo.toml`, `Cargo.lock`, `LICENSE`, `README.md`, and `docs/release.md`
  contain the exact metadata, MIT license, resolved single patch bump, install
  instructions, and private preparation checklist. No `just publish` recipe or
  public-release workflow is added by TASK-037.
- The CLI version test remains dynamic and passes for the resolved release
  version; no stale hard-coded version remains.
- `just qcheck`, `just mac-qcheck`, `cargo package --locked --list`,
  `cargo publish --locked --dry-run`, and the local source install pass without
  uploading to crates.io or creating a release tag.
- The release document clearly defers all registry publication, credentials,
  release-tag creation, and tag push steps to TASK-068, so completing TASK-037
  cannot expose the release to the wider world.

## TASK-067 - add dormant automation for tagged crates.io releases

State: COMPLETED

Goal:

- After TASK-037's private preparation, add the complete but dormant automation
  for later crates.io releases. Keep it fail-closed and incapable of publishing
  until TASK-068 has performed the first manual publication and activated it.

Dependencies:

- TASK-037 must be `COMPLETED` with only private release preparation; the first
  crate publication is explicitly not a dependency of TASK-067.
- No crates.io Trusted Publisher or release environment activation is required
  to complete TASK-067. Those external settings are configured by TASK-068 only
  after the first manual publication.

Scope:

- Add `.github/workflows/release.yml`, triggered only by a pushed stable tag
  matching `^v[0-9]+\.[0-9]+\.[0-9]+$`. Pre-release tags are not accepted. Do
  not use `pull_request_target` or `workflow_run` for the publish job; crates.io
  Trusted Publishing blocks those triggers.
- Require the repository variable `RELEASE_AUTOMATION_ENABLED` to equal the
  exact string `true` before any registry query, OIDC request, or publish step;
  fail with an actionable message otherwise. TASK-068 sets this variable only
  after the first manual publication and trusted-publisher configuration.
- Give the publish job only `contents: read`, `actions: read`, and
  `id-token: write` permissions, reference the `release` environment, and
  serialize releases with a concurrency group that never cancels an in-progress
  publish.
- Check out full history and reject the run unless:
  - the tag is exactly `v<major>.<minor>.<patch>` under the stable-tag regex;
  - the tag's version equals the package version from `Cargo.toml`;
  - the tagged commit is reachable from `origin/main`; and
  - the version endpoint `https://crates.io/api/v1/crates/stay/<version>`
    returns HTTP 404 when the version has not yet been published. If it returns
    HTTP 200, skip authentication and publication and enter verification-only
    bootstrap/recovery mode; any other response fails the run.
- Query `GET /repos/nevdelap/stay/actions/runs?head_sha=<tag-sha>&event=push`
  with the read-only GitHub token, select the `CI` workflow run for that SHA,
  and query its jobs endpoint. Require one completed run with conclusion
  `success` and successful `check`, `msrv`, and `macos` jobs. A missing,
  in-progress, cancelled, or failed CI run must stop the release before
  authentication. This replaces a `workflow_run` dependency while still
  requiring the full main-line CI result.
- Run the Linux release gates, including `just qcheck` and
  `cargo publish --locked --dry-run`, before requesting the OIDC credential. The
  workflow must not bypass verification with `--no-verify` and must not accept a
  dirty checkout.
- When the version endpoint returned HTTP 404, authenticate with
  `rust-lang/crates-io-auth-action@v1` immediately before
  `cargo publish --locked`. Do not add `CARGO_REGISTRY_TOKEN`, a crates.io API
  token, or any other registry credential to GitHub secrets.
- Publish exactly once in publish mode, then poll the version endpoint at most
  60 times at 10-second intervals for HTTP 200. In either publish or
  verification-only mode, install the version into a fresh temporary
  `CARGO_INSTALL_ROOT`, retrying the install at most 12 times at 10-second
  intervals only for registry/index-unavailable errors. Fail immediately on
  build or version errors, run the installed `stay --version`, and fail unless
  it reports exactly `stay <version>`. Upload only non-secret diagnostic results
  as the workflow summary or artifact.
- Extend `docs/release.md` with the dormant-workflow gate, the one-time Trusted
  Publishing setup, the tag-push procedure, and recovery instructions. State
  that the first release remains manual, later releases require a version bump
  and matching `v<version>` tag, and a failed workflow must be investigated
  rather than republished by force.

Acceptance criteria:

- Completing TASK-067 with `RELEASE_AUTOMATION_ENABLED` unset or not equal to
  `true` cannot publish, request OIDC credentials, or change crates.io; its
  implementation and tests remain private.
- Once TASK-068 activates the variable and external trust settings, a matching
  stable version tag on a commit reachable from `main` runs the release
  workflow; branch, pull-request, pre-release, arbitrary-tag, and `workflow_run`
  events cannot publish.
- The workflow rejects tag/version mismatches, non-main history, missing or
  unsuccessful full CI, failed quality or dry-run checks, and unavailable
  crates.io publication before the publish command. An already published version
  enters verification-only mode and is never republished.
- Trusted Publishing uses only the short-lived OIDC exchange with the exact
  configured repository, workflow, and `release` environment; no static
  crates.io token is present.
- The workflow invokes `cargo publish --locked` exactly once only in publish
  mode, verifies the installed published binary's exact version, and has the
  specified 60/12 bounded failure behavior for propagation and installation.
- `docs/release.md` documents private completion, TASK-068 activation, manual
  bootstrap, and automated tagged releases, including the external settings that
  cannot be configured in Git.
- `just qcheck` and `just mac-qcheck` pass, and the release workflow passes the
  repository's YAML/quality checks without performing a public release.

## TASK-068 - publish the first crates.io release and activate automation

State: BLOCKED

Goal:

- After TASK-037's private release preparation and TASK-067's dormant,
  fail-closed workflow are both complete, perform the one-time public bootstrap
  release of `stay`, configure crates.io Trusted Publishing, and hand the
  immutable release tag to the guarded workflow. This is the first task that may
  publish to crates.io, create or push a release tag, or configure external
  release settings.

Dependencies:

- TASK-037 must be `COMPLETED` and must have performed only private metadata,
  packaging, dry-run, and local-source-install checks. Its implementation and
  completion must not have published, tagged, pushed, or configured an external
  publisher.
- TASK-067 must be `COMPLETED`, with its release workflow and tests passing
  while `RELEASE_AUTOMATION_ENABLED` is unset or not equal to `true`.
- The operator must have a crates.io account with a verified email and the
  authority to publish the package name `stay`, and GitHub administration rights
  for the repository, the `release` environment, repository variables, and the
  repository's crates.io Trusted Publisher configuration. The task must explain
  where these rights are needed; no credential or token may be committed.
- Before the public action starts, the intended release commit must be present
  on `origin/main`, the worktree must be clean, and the repository CI and
  private release checks must be green. Resolve the release version from that
  commit; do not assume `0.0.49` if a later patch has already been made.

Scope:

- Add the real `just publish` recipe, and add it only in TASK-068. It must:
  - refuse to run when `CI=true` or `GITHUB_ACTIONS=true`, so the operator
    cannot accidentally turn a local bootstrap command into a CI publication;
  - require a clean worktree and read the single package version from the
    checkout rather than accepting a version argument;
  - run `cargo publish --locked --dry-run` and stop on any failure;
  - query `https://crates.io/api/v1/crates/stay` immediately before publication
    and continue only for HTTP 404, meaning the package name is still unclaimed.
    HTTP 200 or any other response must stop before a real publish;
  - run `cargo publish --locked` exactly once after those checks. It must not
    retry, publish a second time, print credentials, or be called implicitly by
    another recipe. A failed command must be treated as requiring inspection,
    not as permission to blindly rerun it.
- Complete `docs/release.md` as an operator runbook written for someone who has
  never published a Rust crate or has forgotten the process. It must explain the
  difference between a local package, a crates.io crate, a Git tag, and a GitHub
  Actions release; identify which steps are private and which steps make the
  release public; and give an ordered checklist with explicit stop points for:
  - checking tools, account access, verified email, clean worktree, green CI,
    and the resolved package version;
  - capturing the exact immutable release commit before publication, for example
    with `release_commit=$(git rev-parse HEAD)`, then recording and checking
    that SHA rather than relying on a moving branch name;
  - inspecting the package contents with `cargo package --locked --list` and
    running the locked publish dry run before any public action;
  - checking the crates.io package endpoint and requiring HTTP 404 immediately
    before invoking `just publish`, with a plain-language explanation that the
    first registration of a name is first-come and must not be raced or
    repeated;
  - invoking `just publish` once, waiting for its result, and recording whether
    crates.io accepted the upload without exposing a token;
  - polling the exact version endpoint for HTTP 200 at most 60 times with
    10-second intervals, treating persistent 404 and other HTTP failures as stop
    conditions;
  - installing from the registry into a fresh temporary `CARGO_INSTALL_ROOT`
    with `cargo install --locked --version <version> stay`, retrying at most 12
    times with 10-second intervals only for an unavailable index or propagation
    error, failing immediately for compilation, package, or version errors, and
    checking that the installed binary reports exactly `stay <version>`;
  - configuring crates.io Trusted Publishing for repository `nevdelap/stay`,
    workflow `.github/workflows/release.yml`, and GitHub environment `release`,
    then configuring any required environment protection and setting the
    repository variable `RELEASE_AUTOMATION_ENABLED=true` only after the manual
    publication and install verification succeed. The runbook must tell the
    operator not to create a long-lived crates.io token in GitHub secrets;
  - creating an annotated tag pointing to the captured SHA only after the
    registry and install checks succeed, using
    `git tag -a "v$version" "$release_commit" -m "Release $version"`, verifying
    the tag resolves to the captured SHA, and pushing it with
    `git push origin "v$version"` without force. Explain that this push starts
    the workflow and that, because the version is already published, the
    workflow must enter its verification-only path rather than publish again;
  - confirming the tagged workflow completed successfully and recording the tag,
    commit SHA, package version, registry verification, install result, and
    workflow URL for future operators.
- Include a recovery section that explicitly says what to do if publication
  succeeded but polling, installation, tag creation, tag push, or workflow
  activation fails: do not republish, yank, replace, or force-push the tag;
  inspect the crates.io API and workflow logs, fix the failing configuration,
  retry only the safe verification or non-force tag push, and use the workflow's
  already-published verification mode. Explain when to stop and ask a maintainer
  rather than guessing.
- Configure the external crates.io Trusted Publisher and GitHub `release`
  environment only after the manual release is verified. The configuration must
  exactly identify `nevdelap/stay`, `.github/workflows/release.yml`, and
  `release`; the repository variable must remain disabled until this point. Do
  not place a crates.io API token, `cargo login` token, or other secret in the
  repository, workflow YAML, documentation, or logs.
- Do not alter TASK-067's fail-closed behavior to make the bootstrap easier. The
  first annotated tag is expected to find an already published version and take
  the workflow's verification-only path. Later releases require a new patch
  version, a matching stable `v<version>` tag on `main`, and a successful full
  CI result before the workflow can publish.

Acceptance criteria:

- No task before TASK-068 can publish to crates.io, create or push the release
  tag, or configure the external Trusted Publisher. TASK-068 is the sole public
  bootstrap task, and it remains blocked until TASK-037 and TASK-067 are
  complete.
- `just publish` is operator-only, refuses CI execution and dirty worktrees,
  performs the locked dry run and immediate HTTP-404 ownership check, and
  invokes the real locked publish command exactly once with no retry or secret
  leakage.
- The package metadata, package version, package contents, and captured release
  commit are checked before publication; the annotated `v<version>` tag points
  to that exact SHA and is pushed without force only after publication and
  install verification succeed.
- The runbook is complete enough for a first-time or long-lapsed crates.io
  publisher, clearly labels public side effects, provides every command and
  bounded retry/stop rule, explains account and GitHub permissions, and includes
  recovery guidance for partial success.
- Registry propagation is polled at most 60 times at 10-second intervals, and
  registry installation is retried at most 12 times at 10-second intervals only
  for transient availability or propagation failures. The installed binary
  reports exactly the resolved package version.
- Trusted Publishing is configured with short-lived GitHub OIDC for the exact
  repository, workflow, and `release` environment; no static crates.io token is
  stored in GitHub. `RELEASE_AUTOMATION_ENABLED=true` is set only after the
  manual release succeeds and the first tagged workflow is ready to verify.
- The first pushed tag completes in TASK-067's verification-only mode without
  republishing the already published version. Subsequent matching stable tags
  can publish only after all of TASK-067's gates pass.
- `just qcheck`, `just mac-qcheck`, the workflow/YAML quality checks, and the
  locked dry run pass. The actual crates.io publication, external configuration,
  tag push, and observation of the first workflow are documented operator
  actions, not CI tests performed as part of implementation.
