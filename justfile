set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Keep the commit-message formatter's mutable tool cache outside the repository.

UV_CACHE_DIR := "/tmp/stay-uv-cache"

# Show the available recipes when Just is invoked without a recipe.
@_:
    @just --list

help:
    @just --list

install-hooks:
    mkdir -p .git/hooks
    ln -sfn "$(pwd)/scripts/pre-push" .git/hooks/pre-push

install: install-hooks
    cargo fetch --locked

update-rust:
    rustup update

msrv:
    rustup toolchain list | grep -q '^1\.88' || rustup toolchain install 1.88 --profile minimal
    CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +1.88 check --locked

update-lock:
    cargo update --verbose

sweep:
    if command -v cargo-sweep > /dev/null 2>&1; then cargo sweep --time 3; else cargo clean; fi

# Format the current commit message at 60 columns, amending only when needed. GitHub Actions must remain non-mutating, so its format gate skips this step.
_format-commit:
    if [ "${GITHUB_ACTIONS:-}" != "true" ]; then UV_CACHE_DIR={{ UV_CACHE_DIR }} scripts/quality.py commit-message; fi

# scripts/quality.py owns file selection and the formatter/linter matrix.
format scope="changed": _format-commit
    scripts/quality.py format --scope {{ scope }}
    git diff --no-ext-diff --exit-code

format-all:
    just format all

lint scope="changed":
    just format {{ scope }}
    just _assert-clean-worktree
    scripts/quality.py lint --scope {{ scope }}

lint-all:
    just lint all

_assert-clean-worktree:
    git diff --no-ext-diff --exit-code

test:
    cargo test --locked --all-targets --all-features

check scope="changed":
    just lint {{ scope }}
    just test
    just msrv

check-all:
    just check all

mac-check:
    scripts/maccmd cargo test --locked --all-targets --all-features

mac-qcheck: (_q "mac-check")

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

qformat: (_q "format")

qformat-all: (_q "format all")

qlint: (_q "lint")

qlint-all: (_q "lint all")

qtest: (_q "test")

qcheck: (_q "check")

qcheck-all: (_q "check all")

build:
    cargo build --locked

build-release:
    cargo build --locked --release

run *args:
    cargo run --locked -- {{ args }}
