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
