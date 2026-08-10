set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Keep the commit-message formatter's mutable tool cache outside the repository.

UV_CACHE_DIR := "/tmp/stay-uv-cache"

# Show the available recipes when Just is invoked without a recipe.
@_:
    @just --list

help:
    @just --list

# Install the pre-push hook into this checkout.
install-hooks:
    mkdir -p .git/hooks
    ln -sfn "$(pwd)/scripts/pre-push" .git/hooks/pre-push

# Install hooks, nextest, and locked Rust dependencies.
install: install-hooks install-nextest
    cargo fetch --locked

# Install the prebuilt cargo-nextest binary required by the fast test gate.
install-nextest:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cargo-nextest > /dev/null 2>&1; then
        cargo nextest --version
        exit 0
    fi
    case "$(uname -s)" in
        Darwin) platform="mac" ;;
        Linux) platform="linux" ;;
        *) echo "unsupported platform for the prebuilt nextest installer" >&2; exit 1 ;;
    esac
    cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
    mkdir -p "$cargo_bin"
    curl -LsSf "https://get.nexte.st/latest/$platform" | tar zxf - -C "$cargo_bin"
    cargo nextest --version

# Update locally installed Rust toolchains.
update-rust:
    rustup update

# Check the declared minimum supported Rust version.
msrv:
    rustup toolchain list | grep -q '^1\.88' || rustup toolchain install 1.88 --profile minimal
    CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +1.88 check --locked

# Check dependencies for known security advisories. cargo-audit reads

# Cargo.lock directly; unlike Cargo, it has no --locked flag.
audit:
    cargo audit

# Update Cargo.lock using Cargo's resolver.
update-lock:
    cargo update --verbose

# Remove stale artifacts with cargo-sweep when installed; otherwise run cargo clean.
sweep:
    if command -v cargo-sweep > /dev/null 2>&1; then cargo sweep --time 3; else cargo clean; fi

# Format the current commit message at 60 columns, amending only when needed. GitHub Actions must remain non-mutating, so its format gate skips this step.
_format-commit:
    if [ "${GITHUB_ACTIONS:-}" != "true" ]; then UV_CACHE_DIR={{ UV_CACHE_DIR }} scripts/quality.py commit-message; fi

# scripts/quality.py owns file selection and the formatter/linter matrix.

# Format the selected scope; use `all` for the whole repository.
format scope="changed": _format-commit
    scripts/quality.py format --scope {{ scope }}
    git diff --no-ext-diff --exit-code

# Format every tracked file instead of only the selected scope.
format-all:
    just format all

# Format and lint the selected scope.
lint scope="changed":
    just format {{ scope }}
    just _assert-clean-worktree
    scripts/quality.py lint --scope {{ scope }}

# Lint every tracked file instead of only the selected scope.
lint-all:
    just lint all

_assert-clean-worktree:
    git diff --no-ext-diff --exit-code

# Run the standard Cargo test runner across all targets and features.
test:
    cargo test --locked --all-targets --all-features
    just test-publish

# Run all tests with cargo-nextest; cargo-nextest must be installed locally.
test-nextest:
    cargo nextest run --locked --all-targets --all-features
    just test-publish

# Test the operator-only publish orchestration without network access.
test-publish:
    uv run --script scripts/test_publish.py

# Run the fast local loop: changed-file quality and the parallel test runner.
check-fast scope="changed":
    just lint {{ scope }}
    just test-nextest

# Run the full check with nextest, including the MSRV gate.
check-nextest scope="changed":
    just lint {{ scope }}
    just test-nextest
    just msrv

# Run one integration-test target with nextest, e.g. `just test-target attachment`.
test-target target:
    cargo nextest run --locked --all-features --test {{ target }}

# Run tests matching a nextest expression, e.g. `just test-filter picker`.
test-filter filter:
    cargo nextest run --locked --all-features -E 'test({{ filter }})'

# Show worktree changes, quality groups, likely tests, and verification gates.
context:
    uv run --script scripts/dev_context.py

# Perform the one-time operator-only crates.io bootstrap publication.
publish:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "${CI:-}" == "true" || "${GITHUB_ACTIONS:-}" == "true" ]]; then
        echo "publish is operator-only and cannot run in CI" >&2
        exit 1
    fi
    if [[ -n "$(git status --porcelain)" ]]; then
        echo "publish requires a clean worktree, including no untracked files" >&2
        exit 1
    fi

    version="$(
        cargo metadata --format-version 1 --no-deps |
            jq -er '
                if (.packages | length) != 1 then
                    error("expected exactly one package")
                elif .packages[0].name != "stay" then
                    error("expected package stay")
                else
                    .packages[0].version
                end
            '
    )"
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "package version must be a stable semantic version: $version" >&2
        exit 1
    fi

    cargo publish --locked --dry-run

    package_url="https://crates.io/api/v1/crates/stay"
    user_agent="stay-release-bootstrap/0.1 (https://github.com/nevdelap/stay)"
    if ! http_code="$(
        curl --silent --show-error --output /dev/null \
            --header "User-Agent: $user_agent" \
            --write-out '%{http_code}' --connect-timeout 10 --max-time 30 \
            "$package_url"
    )"; then
        echo "could not query crates.io package endpoint" >&2
        exit 1
    fi
    if [[ "$http_code" != "404" ]]; then
        echo "refusing publication: crates.io returned HTTP $http_code" >&2
        exit 1
    fi

    cargo publish --locked

# Run changed-file quality, the standard test suite, and MSRV.
check scope="changed":
    just lint {{ scope }}
    just test
    just msrv

# Run the full-repository standard check.
check-all:
    just check all

# Run the test suite on the configured macOS host.
mac-check:
    scripts/maccmd.sh cargo test --locked --all-targets --all-features

# Run the local acceptance suite.
acceptance:
    #!/usr/bin/env bash
    set -euo pipefail
    artifact_dir="$(mktemp -d "${TMPDIR:-/tmp}/stay-acceptance-artifacts.XXXXXX")"
    trap 'rm -rf -- "$artifact_dir"' EXIT
    cargo build --release --locked --all-features
    target_dir="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
    [[ "$target_dir" == /* ]] || { echo "Cargo metadata returned no absolute target directory" >&2; exit 1; }
    STAY_BIN="$target_dir/release/stay" \
        ACCEPTANCE_ARTIFACT_DIR="$artifact_dir" \
        scripts/ci-run-acceptance.sh

# Run the acceptance suite on the configured macOS host.
mac-acceptance:
    scripts/maccmd.sh bash -lc 'set -euo pipefail; artifact_dir="$(mktemp -d "${TMPDIR:-/tmp}/stay-acceptance-artifacts.XXXXXX")"; trap '\''rm -rf -- "$artifact_dir"'\'' EXIT; cargo build --release --locked --all-features; target_dir="$(cargo metadata --format-version 1 --no-deps | sed -n '\''s/.*"target_directory"[[:space:]]*:[[:space:]]*"\([^"\\]*\)".*/\1/p'\'')"; [[ "$target_dir" == /* ]] || { echo "Cargo metadata returned no absolute target directory" >&2; exit 1; }; STAY_BIN="$target_dir/release/stay" ACCEPTANCE_ARTIFACT_DIR="$artifact_dir" scripts/ci-run-acceptance.sh'

# Build the debug binary.
build:
    cargo build --locked

# Build the optimized release binary.
build-release:
    cargo build --locked --release

# Run the binary with the supplied arguments.
run *args:
    cargo run --locked -- {{ args }}

_q target:
    #!/usr/bin/env bash
    set -uo pipefail
    echo -n "{{ target }}: "
    if just {{ target }} > check.log 2>&1; then
        echo "ok"
    else
        status=$?
        echo "FAILED: {{ target }} — tail of check.log (full log there):" >&2
        tail -n 40 check.log >&2
        exit $status
    fi

# Run format quietly and write failures to check.log.
qformat: (_q "format")

# Run whole-repository formatting quietly.
qformat-all: (_q "format all")

# Run format and lint quietly.
qlint: (_q "lint")

# Run whole-repository linting quietly.
qlint-all: (_q "lint all")

# Run the standard test suite quietly.
qtest: (_q "test")

# Run the fast local check quietly.
qcheck-fast: (_q "check-fast")

# Run the full nextest-based check quietly.
qcheck-nextest: (_q "check-nextest")

# Run the full standard check quietly.
qcheck: (_q "check")

# Run the full-repository standard check quietly.
qcheck-all: (_q "check all")

# Run macOS tests quietly and write details to check.log.
mac-qcheck: (_q "mac-check")

# Run the local acceptance suite quietly.
qacceptance: (_q "acceptance")

# Run the macOS acceptance suite quietly.
mac-qacceptance: (_q "mac-acceptance")
