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

State: REVIEWED_FOUND_ISSUES

Pre-start handoff:

- TASK-068 has not formally started, but draft implementation changes are
  already present in `justfile` and `docs/release.md`, alongside this planning
  entry. They add a provisional `just publish` recipe and a consolidated
  operator runbook, including the repository visibility and `main` ruleset
  steps. No public release, repository visibility change, GitHub ruleset,
  Trusted Publisher configuration, tag push, or automation activation has been
  performed.
- When TASK-068 starts, Igor owns this existing draft work. He must review it
  against the complete task, retain or revise it as appropriate, add or update
  tests and acceptance evidence, and include the resulting implementation in the
  task's normal review and completion process. The draft must not be treated as
  completed TASK-068 work merely because it is already in the worktree. Rufus's
  first review is of this TASK-068 specification and the pre-start handoff,
  before Igor begins implementation. Rufus's later review is of Igor's completed
  implementation and evidence; the first review must not be treated as final
  implementation approval.

Goal:

- After TASK-037's private release preparation and TASK-067's dormant,
  fail-closed workflow are both complete, perform the one-time public bootstrap
  release of `stay`, configure crates.io Trusted Publishing, and hand the
  immutable release tag to the guarded workflow. This is the first task that may
  publish to crates.io, create or push a release tag, or configure external
  release settings.

Completion model:

- Igor's implementation work must provide the guarded command, the complete
  operator checklist, automated coverage for the safe refusal and command-order
  paths, and passing quality evidence. This implementation phase must not make
  the repository public, publish the crate, change GitHub settings, create or
  push a tag, or enable release automation.
- TASK-068 is not complete merely when those files and tests are ready. An
  authorized maintainer must then execute the checklist's public bootstrap and
  record the release evidence: published package, registry installation,
  repository visibility, effective `main` ruleset, Trusted Publishing, enabled
  automation, immutable tag, and successful tagged workflow.
- Igor must not perform any public or externally mutating action himself.
- Before each human-only checkpoint, Igor must amend the single in-progress
  TASK-068 commit with all work and evidence completed so far, run the required
  commit-message and lint checks, and hand that commit to Rufus for an
  in-progress review. Igor must then stop and ask Nev to perform the checkpoint.
  Nev reports the exact result; Igor may resume with read-only verification and
  further amendments to the same commit. No public action may be performed by
  Igor or by an automated test.
- Rufus's first review approves the completeness and clarity of this
  specification before Igor implements it. Rufus's later review evaluates Igor's
  implementation and evidence; neither review authorizes the public actions on
  its own.

Dependencies:

- TASK-037 must be `COMPLETED` and must have performed only private metadata,
  packaging, dry-run, and local-source-install checks. Its implementation and
  completion must not have published, tagged, pushed, or configured an external
  publisher.
- TASK-067 must be `COMPLETED`, with its release workflow and tests passing
  while `RELEASE_AUTOMATION_ENABLED` is unset or not equal to `true`.
- The operator must have a crates.io account with a verified email and the
  authority to publish the package name `stay`, and GitHub administration rights
  for repository visibility, the `main` branch ruleset, the `release`
  environment, repository variables, and the repository's crates.io Trusted
  Publisher configuration. The task must explain where these rights are needed;
  no credential or token may be committed.
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
    with the stable descriptive User-Agent documented in `docs/release.md`, and
    continue only for HTTP 404, meaning the package name is still unclaimed.
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
  - reviewing the private repository for secrets, private material, and other
    content that must not become public, stopping for explicit operator
    confirmation, and then changing the GitHub repository visibility to public;
  - creating or applying a `main` branch ruleset that requires pull requests and
    blocks direct pushes to `main`, then verifying the effective ruleset;
    explain that these are GitHub administrative actions and require repository
    administration rights;
  - creating an annotated tag pointing to the captured SHA only after the
    registry and install checks succeed, using
    `git tag -a "v$version" "$release_commit" -m "Release $version"`, verifying
    the tag resolves to the captured SHA, and pushing it with
    `git push origin "v$version"` without force. Explain that this push starts
    the workflow and that, because the version is already published, the
    workflow must enter its verification-only path rather than publish again;
  - confirming the tagged workflow completed successfully and recording the tag,
    commit SHA, package version, registry verification, install result, and
    workflow URL for future operators. The final document must be one tidy,
    ordered checklist of shell commands, GitHub/crates.io web actions, explicit
    stop points, and evidence to record; remove detailed historical preparation
    sections and completed-task handoff narrative from `docs/release.md`.
- Add automated tests for `just publish` using a hermetic fixture or command
  mocks. The tests must cover CI refusal, dirty-worktree refusal, invalid or
  non-single-package metadata, dry-run failure, network failure, every non-404
  package response, successful command ordering, and the guarantee that the real
  publish command is invoked once only. Tests must never publish to crates.io or
  require external credentials.
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
- Change the GitHub repository from private to public only after the runbook's
  visibility review and explicit confirmation. First preflight whether the
  current GitHub plan supports branch rulesets on this private repository. If it
  does, Nev must apply and verify an active `main` branch ruleset requiring pull
  requests and blocking direct pushes while the repository is still private. If
  it does not, the runbook must stop before visibility changes and require a
  plan upgrade or another explicitly approved control. Visibility may change
  only after protection is effective, and the ruleset must be re-verified after
  the change.
- Mark the private-ruleset preflight, visibility change, one-time publication,
  external Trusted Publishing and automation configuration, and tag
  creation/push as human-only checkpoints. Before each one, Igor must amend the
  task commit with all current work, obtain Rufus's in-progress review, stop,
  and wait for Nev's report before continuing.
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
- The runbook explicitly documents the private-to-public repository transition,
  its review and confirmation stop, and the required GitHub administration
  permission.
- The runbook explicitly documents a verified `main` branch ruleset requiring
  pull requests and blocking direct pushes to `main`, including a
  plan-capability preflight, private-repository verification before visibility
  changes, and post-visibility re-verification. An unsupported private-plan
  capability must stop the release before the repository is made public.
- `docs/release.md` is a single current operator checklist and contains no
  detailed historical TASK-037/TASK-067 preparation sections or stale fixed
  version assumptions.
- Hermetic tests cover every `just publish` refusal path and prove the dry-run,
  package ownership check, descriptive User-Agent, and single real-publish
  ordering without contacting crates.io or exposing credentials.
- The first pushed tag completes in TASK-067's verification-only mode without
  republishing the already published version. Subsequent matching stable tags
  can publish only after all of TASK-067's gates pass.
- `just qcheck`, `just mac-qcheck`, the workflow/YAML quality checks, and the
  locked dry run pass. The actual crates.io publication, external configuration,
  tag push, and observation of the first workflow are Nev-operated actions at
  explicit checkpoints, not actions performed by Igor or by CI. Igor must not
  make the repository public or otherwise cause a public side effect.
