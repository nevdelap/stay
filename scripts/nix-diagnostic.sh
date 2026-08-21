#!/usr/bin/env bash
set -euo pipefail

# This is a Linux diagnostic only. Native CI remains authoritative for all four
# package systems and is the only verification that covers Darwin or ARM.
readonly NIX_VERSION="2.35.2"
readonly NIX_IMAGE="${STAY_NIX_IMAGE:-nixos/nix@sha256:7a007c766426c1877758ddc5cb87a965ac131fc78c582ce0083d922d51ae945c}"
readonly NIX_SYSTEM="${STAY_NIX_SYSTEM:-x86_64-linux}"
readonly NIX_VOLUME="${STAY_NIX_VOLUME:-stay-nix-${NIX_VERSION}}"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT="$repo_root"

if [[ "$NIX_SYSTEM" != "x86_64-linux" ]]; then
    printf 'Docker Nix diagnostic supports only x86_64-linux; use native CI for %s\n' \
        "$NIX_SYSTEM" >&2
    exit 2
fi

command -v docker >/dev/null 2>&1 || {
    printf 'Docker is required for the Nix diagnostic\n' >&2
    exit 1
}

docker run --rm --pull=missing \
    --volume "$REPO_ROOT:/workspace" \
    --volume "$NIX_VOLUME:/nix" \
    --workdir /workspace \
    --env NIX_SYSTEM="$NIX_SYSTEM" \
    "$NIX_IMAGE" \
    bash -eu -o pipefail -c '
        git config --global --add safe.directory /workspace
        nix --extra-experimental-features "nix-command flakes" build \
            --no-link \
            ".#packages.${NIX_SYSTEM}.stay" \
            ".#packages.${NIX_SYSTEM}.default"
        nix --extra-experimental-features "nix-command flakes" \
            flake check --system "$NIX_SYSTEM"
    '
