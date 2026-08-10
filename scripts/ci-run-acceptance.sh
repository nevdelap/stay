#!/usr/bin/env bash
# shellcheck disable=SC2329
set -euo pipefail

: "${STAY_BIN:?STAY_BIN must be set to the release binary}"
if [[ "$STAY_BIN" != /* || ! -x "$STAY_BIN" ]]; then
    echo "STAY_BIN must be an absolute executable path" >&2
    exit 2
fi

artifact_dir="${ACCEPTANCE_ARTIFACT_DIR:-${GITHUB_WORKSPACE:-$PWD}/acceptance-artifacts}"
if [[ "$artifact_dir" != /* || "$artifact_dir" == "/" ]]; then
    echo "ACCEPTANCE_ARTIFACT_DIR must be an absolute non-root path" >&2
    exit 2
fi
mkdir -p "$artifact_dir"
test_output_dir="$artifact_dir/test-output"
if [[ -e "$test_output_dir" ]]; then
    if [[ ! -d "$test_output_dir" ]] ||
        find "$test_output_dir" -mindepth 1 -print -quit | grep -q .; then
        echo "acceptance test-output directory must be empty" >&2
        exit 2
    fi
else
    mkdir "$test_output_dir"
fi

tmux_tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/stay-acceptance.XXXXXX")
export TMUX_TMPDIR="$tmux_tmpdir"
unset TMUX

tmux_path=$(command -v tmux)
export STAY_ACCEPTANCE_TOOL_PATH="${tmux_path%/*}:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"
bats_output="$artifact_dir/bats-output.txt"
acceptance_start_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
SECONDS=0

write_timing_artifacts() {
    local status=$1
    local ansi=$'\033'
    local clean_output="$artifact_dir/bats-output-clean.txt"
    local scenario_timings="$artifact_dir/scenario-timings.tsv"
    local worst_case

    if [[ -f "$bats_output" ]]; then
        sed "s/${ansi}\\[[0-9;]*[[:alpha:]]//g" "$bats_output" >"$clean_output"
        sed -n 's/^[[:space:]]*[✓✗] \(.*\) \[\([0-9][0-9]*\)\]$/\2\t\1/p' \
            "$clean_output" >"$scenario_timings"
    else
        : >"$clean_output"
        : >"$scenario_timings"
    fi

    {
        printf 'status=%s\n' "$status"
        printf 'started_at=%s\n' "$acceptance_start_utc"
        printf 'elapsed_seconds=%s\n' "$SECONDS"
        printf 'timing_unit=milliseconds\n'
        printf 'scenario_timings=%s\n' "$scenario_timings"
        if [[ -s "$scenario_timings" ]]; then
            worst_case="$(sort -nr -k1,1 "$scenario_timings" | head -n 1)"
            printf 'worst_case_ms=%s\n' "${worst_case%%$'\t'*}"
            printf 'worst_case_scenario=%s\n' "${worst_case#*$'\t'}"
        else
            printf 'worst_case_ms=unavailable\n'
            printf 'worst_case_scenario=unavailable\n'
        fi
    } >"$artifact_dir/timing-summary.txt"
}

cleanup() {
    local status=$1
    trap - EXIT INT TERM
    set +e
    write_timing_artifacts "$status"
    scripts/ci-acceptance-cleanup.sh "$STAY_BIN" "$TMUX_TMPDIR" \
        >"$artifact_dir/cleanup.log" 2>&1
    if tmux -L stay -f /dev/null list-sessions >/dev/null 2>&1; then
        tmux -L stay -f /dev/null list-sessions >>"$artifact_dir/cleanup.log" 2>&1
        tmux -L stay -f /dev/null kill-server >>"$artifact_dir/cleanup.log" 2>&1
    fi
    rm -rf -- "$TMUX_TMPDIR"
    exit "$status"
}

trap 'cleanup "$?"' EXIT
trap 'cleanup 130' INT
trap 'cleanup 143' TERM

set +e
TERM=xterm bats --timing --formatter pretty --print-output-on-failure \
    --gather-test-outputs-in "$test_output_dir" tests/acceptance.bats |
    tee "$bats_output"
status=${PIPESTATUS[0]}
set -e
exit "$status"
