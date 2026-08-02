# stay release runbook

This is the TASK-068 one-time public bootstrap checklist. It assumes the private
preparation, dormant release workflow, full CI, package checks, and local
installation checks are already complete.

The local package is the archive Cargo builds in the checkout. The crates.io
crate is the public registry copy. The Git tag is an immutable pointer to the
release commit. The GitHub Actions workflow is the automated process triggered
by pushing that tag; it is not the same thing as either the crate or the tag.

Do not skip a **STOP** item. Record the version, commit SHA, command results,
web configuration, tag, and workflow URL as you proceed. Never put a token or
credential in the repository, a GitHub secret, or a log.

## Agent and human boundary

This checklist is completed cooperatively by Igor, Rufus, and Nev. Igor may
implement the private recipe and tests, run private quality gates, and perform
read-only verification. Igor must not publish the crate, change repository
visibility, change GitHub rulesets or environments, configure Trusted
Publishing, enable the automation variable, create a release tag, or push a tag.

Before each human-only checkpoint, Igor must amend the single in-progress
TASK-068 commit with all current work and evidence, run the required
commit-message and gitlint checks, and hand that commit to Rufus for an
in-progress review. Igor then stops and asks Nev to perform the listed action.
Igor resumes only after Nev reports the exact result, and may then add read-only
verification evidence by amending the same commit.

The human-only checkpoints are:

1. the private-plan preflight, private `main` ruleset, visibility change, and
   post-visibility ruleset verification in step 4;
2. the one-time `just publish` invocation in step 6;
3. Trusted Publishing and automation configuration in step 9; and
4. annotated tag creation and tag push in step 10.

If a human action fails, Igor may diagnose it and perform safe read-only checks,
but must not retry a publication, force a tag, or change public settings.

## TASK-068 checklist

### 1. Verify access and the release checkout ✅

You need:

- a crates.io account with a verified email and authority to publish `stay`;
- GitHub repository administration rights to change visibility and manage
  rulesets, environments, variables, and Trusted Publishing; and
- a clean checkout containing the intended release commit.

From that checkout, run:

```sh
set -euo pipefail
command -v cargo jq curl just git
git fetch origin main --no-tags
test -z "$(git status --porcelain)"
release_commit=$(git rev-parse HEAD)
git merge-base --is-ancestor "$release_commit" origin/main
git cat-file -e "$release_commit:.github/workflows/release.yml"
version="$(
    cargo metadata --format-version 1 --no-deps |
    jq -er 'if (.packages | length) != 1 then error("expected one package") elif .packages[0].name != "stay" then error("expected stay") else .packages[0].version end'
)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
printf 'release_commit=%s\nversion=%s\n' "$release_commit" "$version"
```

Record `release_commit` and `version`. Do not substitute a moving branch name
later.

### 2. Run the private release checks ✅

Inspect the package and run all local gates. These commands do not publish:

```sh
cargo package --locked --list
just qcheck
just mac-qcheck
cargo publish --locked --dry-run
```

Verify that CI for `release_commit` is green, including the required `check`,
`msrv`, and `macos` jobs. If any check fails, **STOP** and fix it before any
public action.

### 3. Review the repository before making it public ✅

Inspect the complete current tree and tracked history for credentials, private
customer data, internal-only notes, generated artifacts, or anything else that
must not be disclosed. Remove or escalate anything inappropriate. Do not assume
a later rewrite will make an accidental disclosure harmless.

**STOP — HUMAN ACTION REQUIRED:** Igor must amend the in-progress commit, obtain
Rufus's review, and stop. Nev must then confirm that the repository may be made
public. Changing visibility is an irreversible public disclosure risk and
requires GitHub repository administration rights.

### 4. Preflight protection, make the repository public, and re-verify `main` — HUMAN ACTION ✅

Before changing visibility, Nev must verify that the current GitHub plan
supports branch rulesets for this private repository. GitHub documents branch
and tag rulesets as available on public repositories with GitHub Free, and on
private repositories with GitHub Pro, Team, or Enterprise; verify the current
plan and documentation rather than assuming either capability. See
[GitHub's ruleset plan documentation](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets).

If private branch rulesets are supported, Nev must create or apply the active
`main` ruleset while the repository is still private. If they are not supported,
**STOP**: do not change visibility; upgrade the plan or obtain an explicitly
approved alternative control before continuing.

Immediately open **Settings** → **Rules** → **Rulesets** and create or apply an
active ruleset targeting `main` with:

- pull requests required for updates to `main`; and
- direct pushes to `main` blocked.

Keep bypasses limited to the minimum actors required by the operating model; do
not allow ordinary direct pushes. Verify on the Rulesets page or through
GitHub's rule-evaluation/API view that the ruleset targets `main`, is active,
requires pull requests, and rejects a direct push. Confirm the release workflow
still has the required environment and permissions. **STOP** if the private
ruleset is not effective.

Only after the private ruleset is effective, Nev opens `nevdelap/stay` →
**Settings** → **General** → **Danger Zone** → **Change repository visibility**
→ **Public**. Confirm the warning and record the resulting visibility and
operator/date. Re-open the Rulesets page or GitHub's rule-evaluation/API view
and verify that the same active `main` ruleset still requires pull requests and
rejects a direct push. **STOP** if post-visibility verification fails.

### 5. Check that the package name is unclaimed ✅

Immediately before the one-time publication, run:

```sh
package_status="$(curl --silent --show-error --output /dev/null \
    --header 'User-Agent: stay-release-bootstrap/0.1 (https://github.com/nevdelap/stay)' \
    --write-out '%{http_code}' --connect-timeout 10 --max-time 30 \
    https://crates.io/api/v1/crates/stay)"
test "$package_status" = 404
```

Only HTTP 404 permits continuing. HTTP 200 or any other response is a **STOP**:
the name may already be claimed or the registry cannot be trusted to answer. Do
not race another registration or retry a real publication blindly.

### 6. Publish once — HUMAN ACTION ✅

Igor must amend the in-progress commit with the completed private checks, obtain
Rufus's in-progress review, and stop. Nev must run the guarded operator recipe
exactly once and report its result:

```sh
just publish
```

It repeats the clean-tree, locked dry-run, and package-name checks, refuses CI,
and invokes `cargo publish --locked` exactly once. Record whether crates.io
accepted the upload. If the command fails after contacting crates.io, **STOP**;
inspect the registry before taking any further action. Never rerun it blindly.

### 7. Verify registry propagation ✅

Poll the exact version endpoint no more than 60 times, sleeping 10 seconds
between attempts. HTTP 200 succeeds; persistent 404 or another HTTP failure
stops the release:

```sh
set -euo pipefail
registry_url="https://crates.io/api/v1/crates/stay/$version"
for attempt in $(seq 1 60); do
    status="$(curl --silent --show-error --output /dev/null \
        --header 'User-Agent: stay-release-bootstrap/0.1 (https://github.com/nevdelap/stay)' \
        --write-out '%{http_code}' --connect-timeout 10 --max-time 30 \
        "$registry_url")"
    if [[ "$status" == 200 ]]; then
        echo "registry version available on attempt $attempt"
        break
    fi
    if [[ "$status" != 404 || "$attempt" == 60 ]]; then
        echo "registry verification stopped with HTTP $status" >&2
        exit 1
    fi
    sleep 10
done
```

### 8. Verify a fresh registry installation ✅

Use a fresh install root. Retry at most 12 times at 10-second intervals only for
an unavailable index or registry propagation error. Compilation, package, and
version errors are immediate stops:

```sh
set -euo pipefail
install_root="$(mktemp -d)"
trap 'rm -rf -- "$install_root"' EXIT
for attempt in $(seq 1 12); do
    rm -rf -- "$install_root"
    mkdir -p "$install_root"
    log_file="${TMPDIR:-/tmp}/stay-install-$attempt.log"
    if CARGO_INSTALL_ROOT="$install_root" cargo install --locked \
        --version "$version" stay >"$log_file" 2>&1; then
        break
    fi
    if ! grep -Eiq \
        'failed to (download|fetch|get)|could not (resolve|connect)|connection (reset|refused)|timed out|timeout|spurious network error|HTTP (429|500|502|503|504)' \
        "$log_file"; then
        cat "$log_file" >&2
        exit 1
    fi
    if [[ "$attempt" == 12 ]]; then
        cat "$log_file" >&2
        exit 1
    fi
    sleep 10
done
test "$("$install_root/bin/stay" --version)" = "stay $version"
```

### Evidence recorded through step 8

Record this evidence before starting step 9. The operator was Nev on 2026-08-02
UTC.

- Immutable release commit: `66c918ec1effc162d6c3a90ddd63840e36bff95c`; this
  exact SHA is on `origin/main`.
- Resolved and published package: `stay` version `0.0.49`.
- CI for the release SHA passed in [run 30737198213](https://github.com/nevdelap/stay/actions/runs/30737198213):
  `check`, `msrv`, and `macos` all succeeded. The maintainer-approved R003
  exception remains recorded for the intermittent exact `just qcheck` failure.
- Repository visibility: GitHub read-only verification reported
  `private=false` and `visibility=public`.
- `main` protection: GitHub reported the active branch ruleset `main` with
  `target=branch` and `enforcement=active`; Nev confirmed that the ruleset is
  enabled in the GitHub UI after the visibility change.
- Package ownership preflight returned HTTP 404 for
  `https://crates.io/api/v1/crates/stay`.
- Publication verification returned version `0.0.49`, created at
  `2026-08-02T07:15:47.530732Z`, with `yanked=false`; the final `just publish`
  invocation succeeded after the crates.io account email was verified.
- Fresh registry installation passed with
  `cargo install --locked --version 0.0.49 stay`, and the installed binary
  reported exactly `stay 0.0.49`.

### 9. Configure Trusted Publishing and enable automation — HUMAN ACTION

After publication and installation verification succeed, Nev must, in crates.io
account settings, add a Trusted Publisher with exactly:

- repository: `nevdelap/stay`;
- workflow filename: `release.yml` (the repository path is
  `.github/workflows/release.yml`); and
- GitHub environment: `release`.

Before enabling automation, create the matching GitHub environment. In
`nevdelap/stay`, open **Settings** → **Environments** → **New environment**, name
it exactly `release`, and save it. Configure its protection rules as follows:

- add at least one independent designated maintainer as a required reviewer;
- enable **Prevent self-review** when that option is available;
- restrict deployment branches and tags to the release-tag pattern `v*`, not
  arbitrary branches or tags; and
- add no environment secrets. Trusted Publishing uses GitHub OIDC through the
  workflow's `id-token: write` permission.

Save the environment and verify that `.github/workflows/release.yml` declares
`environment: release` and requests `id-token: write`. Then add the repository
variable `RELEASE_AUTOMATION_ENABLED` with value `true`. Do not create a
long-lived crates.io token, use `cargo login` in CI, or store a registry
credential in GitHub secrets. **STOP** if any Trusted Publishing value or
environment setting does not match exactly.

### 10. Tag the verified commit and start the workflow — HUMAN ACTION

Igor must amend the in-progress commit with the verified configuration, obtain
Rufus's in-progress review, and stop. Nev must then create an annotated tag at
the recorded immutable SHA and push it without force:

```sh
set -euo pipefail
git tag -a "v$version" "$release_commit" -m "Release $version"
test "$(git rev-parse "v$version^{commit}")" = "$release_commit"
git push origin "v$version"
```

The tag push starts the release workflow. Since this version is already on
crates.io, the workflow must take its verification-only path and must not
publish again.

### 11. Confirm completion and record evidence

In GitHub → **Actions**, open the workflow run triggered by `v<version>` and
confirm it completed successfully. Record the tag, commit SHA, package version,
registry verification, installation result, Trusted Publishing configuration,
repository visibility, ruleset name/result, and workflow URL.

### 12. Require Trusted Publishing for future versions — HUMAN ACTION

Only after the tagged Trusted Publishing workflow has completed successfully,
Nev may enable **Require trusted publishing for all new versions** in the
crates.io crate settings. This disables traditional API-token publication and
leaves Trusted Publishing as the only path for future versions. Leave it
unchecked if the workflow has not yet succeeded or if a manual fallback is
still required. Record the setting and operator/date; **STOP** if the setting
does not match the intended release policy.

## Recovery

If publication succeeded but polling, installation, tag creation, tag push, or
workflow activation fails, do not republish, yank, replace, or force-push.
Inspect the crates.io API and workflow logs. Retry only safe verification or a
non-force tag push when the tag does not already exist. Use the workflow's
already-published verification mode. Stop and ask a maintainer if registry
state, tag target, account authority, or Trusted Publishing configuration is
uncertain.
