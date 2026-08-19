#!/usr/bin/env bash
set -euo pipefail

readonly MANDOC_VERSION="1.14.6"
readonly MANDOC_URL="https://mandoc.bsd.lv/snapshots/mandoc-${MANDOC_VERSION}.tar.gz"
readonly MANDOC_SHA256="8bf0d570f01e70a6e124884088870cbed7537f36328d512909eb10cd53179d9c"
readonly MANDOC_IMAGE="${STAY_MANDOC_IMAGE:-stay-mandoc:1.14.6}"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT="$repo_root"

usage() {
    printf 'usage: %s {format|lint}\n' "$0" >&2
    exit 2
}

require_docker() {
    command -v docker >/dev/null 2>&1 || {
        printf 'Docker is required to run pinned mandoc %s\n' "$MANDOC_VERSION" >&2
        exit 1
    }
}

ensure_image() {
    require_docker
    if ! docker image inspect "$MANDOC_IMAGE" >/dev/null 2>&1; then
        docker build --pull --tag "$MANDOC_IMAGE" "$REPO_ROOT/docker/mandoc"
    fi
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$1" | awk '{print $NF}'
    else
        printf 'a SHA-256 utility is required (sha256sum, shasum, or openssl)\n' >&2
        exit 1
    fi
}

cache_root="${STAY_MANDOC_CACHE_DIR:-${TMPDIR:-/tmp}/stay-mandoc-cache}"
archive="$cache_root/mandoc-${MANDOC_VERSION}.tar.gz"
source_root="$cache_root/mandoc-${MANDOC_VERSION}"
binary="$source_root/mandoc"
version_marker="$source_root/.version"

verify_source_version() {
    [[ -f "$1/Makefile" ]] || return 1
    [[ "$(sed -n 's/^VERSION = //p' "$1/Makefile")" == "$MANDOC_VERSION" ]]
}

mkdir -p "$cache_root"
if [[ ! -f "$archive" ]]; then
    curl --fail --location --silent --show-error --retry 3 \
        --connect-timeout 10 --max-time 120 "$MANDOC_URL" --output "$archive"
fi

if [[ "$(sha256 "$archive")" != "$MANDOC_SHA256" ]]; then
    printf 'mandoc source checksum mismatch for %s\n' "$archive" >&2
    exit 1
fi

marker_version=""
marker_binary_sha256=""
if [[ -f "$version_marker" ]]; then
    read -r marker_version marker_binary_sha256 <"$version_marker"
fi

# Migrate the original one-line cache marker without rebuilding an otherwise
# valid pinned tool. This keeps existing developer caches usable on hosts that
# have the runtime zlib library but not its compiler headers.
if [[ -x "$binary" ]] && verify_source_version "$source_root" &&
    [[ "$marker_version" == "$MANDOC_VERSION" ]] &&
    [[ -z "$marker_binary_sha256" ]]; then
    marker_binary_sha256="$(sha256 "$binary")"
fi

if [[ ! -x "$binary" ]] || ! verify_source_version "$source_root" ||
    [[ "$marker_version" != "$MANDOC_VERSION" ]] ||
    [[ ! "$marker_binary_sha256" =~ ^[[:xdigit:]]{64}$ ]]; then
    require_docker
    build_root="$cache_root/.mandoc-build.$$"
    rm -rf -- "$build_root"
    mkdir -p "$build_root"
    trap 'rm -rf -- "$build_root"' EXIT
    tar -xzf "$archive" -C "$build_root"
    build_source="$build_root/mandoc-${MANDOC_VERSION}"
    ensure_image
    docker run --rm \
        --user "$(id -u):$(id -g)" \
        --volume "$build_root:/src" \
        --workdir "/src/mandoc-${MANDOC_VERSION}" \
        "$MANDOC_IMAGE" \
        bash -eu -o pipefail -c './configure && make'
    rm -rf -- "$source_root"
    mv -- "$build_source" "$source_root"
    marker_version="$MANDOC_VERSION"
    marker_binary_sha256="$(sha256 "$binary")"
    printf '%s %s\n' "$marker_version" "$marker_binary_sha256" >"$version_marker"
    trap - EXIT
    rm -rf -- "$build_root"
fi

ensure_image

if [[ "$marker_version" != "$MANDOC_VERSION" ]]; then
    printf 'mandoc tool version mismatch; expected %s\n' "$MANDOC_VERSION" >&2
    exit 1
fi
if ! verify_source_version "$source_root" ||
    [[ "$(sha256 "$binary")" != "$marker_binary_sha256" ]]; then
    printf 'mandoc tool version or binary verification mismatch; expected %s\n' \
        "$MANDOC_VERSION" >&2
    exit 1
fi

run_mandoc() {
    docker run --rm \
        --user "$(id -u):$(id -g)" \
        --volume "$REPO_ROOT:/workspace" \
        --volume "$source_root:/tool:ro" \
        --workdir /workspace \
        "$MANDOC_IMAGE" \
        bash -eu -o pipefail -c 'export LC_ALL=C; exec /tool/mandoc "$@"' \
        -- "$@"
}

case "${1:-}" in
    format)
        mkdir -p "$REPO_ROOT/target/man"
        temporary_output="$REPO_ROOT/target/man/stay.1.txt.tmp.$$"
        trap 'rm -f -- "$temporary_output"' EXIT
        run_mandoc -T utf8 -O width=80 -W warning /workspace/docs/stay.1 \
            >"$temporary_output"
        test -s "$temporary_output"
        mv -- "$temporary_output" "$REPO_ROOT/target/man/stay.1.txt"
        trap - EXIT
        ;;
    lint)
        run_mandoc -T lint /workspace/docs/stay.1
        ;;
    *)
        usage
        ;;
esac
