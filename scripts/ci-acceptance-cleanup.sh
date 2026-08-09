#!/usr/bin/env bash
set -euo pipefail

if (($# < 2)); then
    echo "usage: $0 STAY_BIN TMUX_TMPDIR [SESSION...]" >&2
    exit 2
fi

stay_bin=$1
tmux_tmpdir=$2
shift 2

if [[ "$stay_bin" != /* || ! -x "$stay_bin" ]]; then
    echo "STAY_BIN must be an absolute executable path" >&2
    exit 2
fi

tmp_root=${TMPDIR:-/tmp}
tmp_root=$(cd "$tmp_root" && pwd -P)
if [[ ! -d "$tmux_tmpdir" ]]; then
    echo "TMUX_TMPDIR must be an existing temporary directory" >&2
    exit 2
fi
resolved_tmpdir=$(cd "$tmux_tmpdir" && pwd -P)
case "$resolved_tmpdir" in
    "$tmp_root"/stay-acceptance.*) ;;
    *)
        echo "TMUX_TMPDIR is not a stay acceptance directory" >&2
        exit 2
        ;;
esac

export TMUX_TMPDIR="$tmux_tmpdir"
unset TMUX

bounded_capture() {
    local output_path=$1
    shift
    "$@" >"$output_path" 2>/dev/null &
    local child=$!
    local attempt
    for attempt in {1..50}; do
        : "$attempt"
        if ! kill -0 "$child" 2>/dev/null; then
            wait "$child" 2>/dev/null || :
            return 0
        fi
        sleep 0.1
    done
    kill -TERM "$child" 2>/dev/null || :
    sleep 0.1
    kill -KILL "$child" 2>/dev/null || :
    wait "$child" 2>/dev/null || :
    return 1
}

bounded_kill() {
    "$stay_bin" kill "$1" >/dev/null 2>&1 &
    local child=$!
    local attempt
    for attempt in {1..50}; do
        if ! kill -0 "$child" 2>/dev/null; then
            wait "$child" 2>/dev/null || :
            return 0
        fi
        sleep 0.1
    done
    kill -TERM "$child" 2>/dev/null || :
    sleep 0.1
    kill -KILL "$child" 2>/dev/null || :
    wait "$child" 2>/dev/null || :
}

probe="$(mktemp "$tmux_tmpdir/acceptance-cleanup.XXXXXX")"
trap 'rm -f -- "$probe"' EXIT
if bounded_capture "$probe" "$stay_bin" list --json; then
    discovered=$(tr ',' '\n' <"$probe" | sed -n 's/.*"name":"\([^"]*\)".*/\1/p')
else
    discovered=""
fi

while IFS= read -r session; do
    [[ -n "$session" ]] || continue
    bounded_kill "$session"
done <<<"$discovered"
for session in "$@"; do
    [[ -n "$session" ]] || continue
    bounded_kill "$session"
done
