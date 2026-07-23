set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

format:
    cargo fmt --all
    git diff --exit-code

lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets --all-features

check: format lint test

qformat:
    rm -f check.log
    cargo fmt --all -- --check > check.log 2>&1

qlint:
    cargo clippy --all-targets --all-features -- -D warnings >> check.log 2>&1

qtest:
    cargo test --all-targets --all-features >> check.log 2>&1

qcheck: qformat qlint qtest

mac-qcheck:
    rm -f check.log
    scripts/maccmd cargo test --all-targets --all-features > check.log 2>&1
