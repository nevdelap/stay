set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

format:
    cargo fmt --all

lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets --all-features

check: format lint test

qformat:
    cargo fmt --all -- --check > check.log 2>&1

qlint:
    cargo clippy --all-targets --all-features -- -D warnings > check.log 2>&1

qtest:
    cargo test --all-targets --all-features > check.log 2>&1

qcheck:
    rm -f check.log
    cargo fmt --all -- --check > check.log 2>&1
    cargo clippy --all-targets --all-features -- -D warnings >> check.log 2>&1
    cargo test --all-targets --all-features >> check.log 2>&1
