#!/usr/bin/env bash
set -euo pipefail

: "${STAY_BIN:?STAY_BIN must be set to the release binary}"
if [[ "$STAY_BIN" != /* || ! -x "$STAY_BIN" ]]; then
    echo "STAY_BIN must be an absolute executable path" >&2
    exit 2
fi

tmux_tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/stay-acceptance.XXXXXX")
export TMUX_TMPDIR="$tmux_tmpdir"
unset TMUX

tmux_path=$(command -v tmux)
export STAY_ACCEPTANCE_TOOL_PATH="${tmux_path%/*}:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"

cleanup() {
    local status=$1
    trap - EXIT INT TERM
    set +e
    scripts/ci-acceptance-cleanup.sh "$STAY_BIN" "$TMUX_TMPDIR"
    if tmux -L stay -f /dev/null list-sessions >/dev/null 2>&1; then
        tmux -L stay -f /dev/null kill-server >/dev/null 2>&1
    fi
    rm -rf -- "$TMUX_TMPDIR"
    exit "$status"
}

trap 'cleanup "$?"' EXIT
trap 'cleanup 130' INT
trap 'cleanup 143' TERM

TERM=xterm bats --formatter pretty tests/acceptance.bats
