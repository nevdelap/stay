set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Docker images for file-format and lint checks.
ACTIONLINT_IMAGE := "rhysd/actionlint:latest"
GITLINT_IMAGE := "jorisroovers/gitlint:latest"
HADOLINT_IMAGE := "hadolint/hadolint:latest"
JQ_IMAGE := "ghcr.io/jqlang/jq:latest"
MARKDOWNLINT_IMAGE := "ghcr.io/igorshubovych/markdownlint-cli:latest"
MDFORMAT_IMAGE := "stay-mdformat:latest"
SHELLCHECK_IMAGE := "koalaman/shellcheck:stable"
SHFMT_IMAGE := "mvdan/shfmt:v3"
TAPLO_IMAGE := "tamasfe/taplo:latest"
YAMLFMT_IMAGE := "ghcr.io/google/yamlfmt:latest"
YAMLLINT_IMAGE := "ghcr.io/ffurrer2/yamllint:latest"
# Keep Buildx's mutable activity state outside the repository and build output.
BUILDX_CONFIG := "/tmp/stay-buildx"
UV_CACHE_DIR := "/tmp/stay-uv-cache"
UV_TOOL_DIR := "/tmp/stay-uv-tools"

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

# Format Bash scripts with dockerized shfmt. Files under scripts/ are included
# even when they do not use a .sh suffix (for example scripts/pre-push).
_format-bash:
    find . -type f \( -name '*.sh' -o -path './scripts/*' \) -not -name '*.py' -not -path './.git/*' -not -path './target*' -exec sh -c 'docker run --pull always --rm -i -v "$(pwd)":/workdir -w /workdir {{ SHFMT_IMAGE }} -w -i 4 -ci "$1"' _ {} \;

# Format the current commit message at 60 columns, amending only when needed.
# GitHub Actions must remain non-mutating, so its format gate skips this step.
_format-commit:
    if [ "${GITHUB_ACTIONS:-}" != "true" ]; then UV_CACHE_DIR={{ UV_CACHE_DIR }} scripts/format_commit.py; fi

# Apply safe Dockerfile formatting fixes with tally.
_format-docker:
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run --from tally-cli tally lint --fix --fail-level none --slow-checks off --ignore hadolint/DL3007 --ignore tally/prefer-package-cache-mounts $(find . -type f -name 'Dockerfile*' -not -path './.git/*' -not -path './target*')

# Format the Justfile.
_format-just:
    just --fmt --unstable

# Format tracked JSON files with dockerized jq.
_format-json:
    git ls-files -z '*.json' | xargs -0 -r -n1 sh -c 'tmp=$(mktemp); docker run --rm -i -v "$(pwd)":/workdir -w /workdir {{ JQ_IMAGE }} --sort-keys . "$1" > "$tmp"; mv "$tmp" "$1"' _

# Format Markdown with mdformat and its table/frontmatter plugins.
_format-markdown:
    BUILDX_CONFIG={{ BUILDX_CONFIG }} docker build -q -t {{ MDFORMAT_IMAGE }} docker/mdformat
    docker run --rm -u "$(id -u):$(id -g)" -v "$(pwd)":/workdir -w /workdir {{ MDFORMAT_IMAGE }} $(find . -type f -name '*.md' -not -path './.git/*' -not -path './target*' -not -path './review_docs/*')

# Format Python scripts with Ruff.
_format-python:
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run pyupgrade --py39-plus --exit-zero-even-if-changed scripts/*.py
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run ruff check --fix scripts
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run ruff format scripts

# Format Rust with rustfmt.
_format-rust:
    cargo fmt --all

# Format TOML with taplo, excluding mutable build output.
_format-toml:
    docker run --pull always --rm -u "$(id -u):$(id -g)" -v "$(pwd)":/workdir -w /workdir {{ TAPLO_IMAGE }} format $(find . -type f -name '*.toml' -not -path './.git/*' -not -path './target*')

# Format YAML with yamlfmt; its configuration honors .gitignore.
_format-yaml:
    docker run --pull always --rm -u "$(id -u):$(id -g)" -v "$(pwd)":/workdir -w /workdir {{ YAMLFMT_IMAGE }} yamlfmt .

format: _format-bash _format-commit _format-docker _format-json _format-just _format-markdown _format-python _format-rust _format-toml _format-yaml
    git diff --no-ext-diff --exit-code

# Lint GitHub Actions workflows with actionlint.
_lint-actionlint:
    docker run --pull always --rm -v "$(pwd)":/repo -w /repo {{ ACTIONLINT_IMAGE }}

# Lint Bash scripts with dockerized shellcheck.
_lint-bash:
    find . -type f \( -name '*.sh' -o -path './scripts/*' \) -not -name '*.py' -not -path './.git/*' -not -path './target*' -exec sh -c 'docker run --pull always --rm -i -v "$(pwd)":/workdir -w /workdir {{ SHELLCHECK_IMAGE }} --external-sources "$1"' _ {} \;

# Lint the latest commit message with gitlint.
_lint-commit:
    docker run --pull always --rm -v "$(pwd)":/repo -w /repo {{ GITLINT_IMAGE }} --config .gitlint

# Lint Dockerfiles with Hadolint.
_lint-docker:
    find . -type f -name 'Dockerfile*' -not -path './.git/*' -not -path './target*' -exec sh -c 'docker run --pull always --rm -i {{ HADOLINT_IMAGE }} /bin/hadolint --ignore DL3007 - < "$1"' _ {} \;

# Lint tracked JSON files with dockerized jq.
_lint-json:
    git ls-files -z '*.json' | xargs -0 -r -n1 sh -c 'docker run --rm -i -v "$(pwd)":/workdir -w /workdir {{ JQ_IMAGE }} empty "$1"' _

# Lint Markdown with markdownlint.
_lint-markdown:
    docker run --pull always --rm -u "$(id -u):$(id -g)" -v "$(pwd)":/workdir {{ MARKDOWNLINT_IMAGE }} /workdir --ignore review_docs

_lint-no-stray-debugging:
    if rg -n 'dbg!|todo!|unimplemented!|print!|eprint!' src tests; then exit 1; fi

# Lint Python scripts with Ruff.
_lint-python:
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run ruff check scripts
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run ty check scripts
    UV_CACHE_DIR={{ UV_CACHE_DIR }} UV_TOOL_DIR={{ UV_TOOL_DIR }} uv tool run bandit -q -r scripts -c .bandit.yml

# Lint Rust with clippy.
_lint-rust:
    cargo clippy --locked --all-targets --all-features -- -D warnings

# Lint TOML with taplo, excluding mutable build output.
_lint-toml:
    docker run --pull always --rm -u "$(id -u):$(id -g)" -v "$(pwd)":/workdir -w /workdir {{ TAPLO_IMAGE }} check $(find . -type f -name '*.toml' -not -path './.git/*' -not -path './target*')

# Lint YAML with yamllint; its configuration honors .gitignore.
_lint-yaml:
    docker run --pull always --rm -u "$(id -u):$(id -g)" -v "$(pwd)":/workdir {{ YAMLLINT_IMAGE }} /workdir

_assert-clean-worktree:
    git diff --no-ext-diff --exit-code

lint: format _assert-clean-worktree _lint-actionlint _lint-bash _lint-commit _lint-docker _lint-json _lint-markdown _lint-no-stray-debugging _lint-python _lint-rust _lint-toml _lint-yaml

test:
    cargo test --locked --all-targets --all-features

check: format lint test msrv

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

qlint: (_q "lint")

qtest: (_q "test")

qcheck: qformat qlint qtest

build:
    cargo build --locked

build-release:
    cargo build --locked --release

run *args:
    cargo run --locked -- {{ args }}
