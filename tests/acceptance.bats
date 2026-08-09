#!/usr/bin/env bats

bats_require_minimum_version 1.14.0

load helpers/acceptance_pty.bash

stay() {
    "$STAY_BIN" "$@"
}

setup_file() {
    : "${STAY_BIN:?STAY_BIN must be set to the release binary}"
    : "${TMUX_TMPDIR:?ci-run-acceptance.sh must provide TMUX_TMPDIR}"
    [[ -x "$STAY_BIN" ]]
    [[ -d "$TMUX_TMPDIR" ]]

    export TMUX_TMPDIR
    unset TMUX
    export ACCEPTANCE_CLEANUP="$BATS_TEST_DIRNAME/../scripts/ci-acceptance-cleanup.sh"
    export ACCEPTANCE_TOOL_PATH="${STAY_ACCEPTANCE_TOOL_PATH:-/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin}"
    export STAY_BIN_DIR="${STAY_BIN%/*}"
}

setup() {
    unset STAY_CMD
    unset STAY_DETACH_KEY
    unset STAY_COPY_MODE_KEY
    unset STAY_HISTORY_LINES
    unset STAY_LOG_CAPTURE_INTERVAL_SECONDS
    unset STAY_SESSION_NAME
    unset TMUX

    export HOME="$BATS_TEST_TMPDIR/home"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/config"
    export SHELL=/bin/sh
    export PATH="$ACCEPTANCE_TOOL_PATH:$STAY_BIN_DIR"
    unset TERM
    mkdir -p "$HOME" "$XDG_CONFIG_HOME"
    [[ ! -e "$HOME/.tmux.conf" ]]

    run_dir="$(mktemp -d "$BATS_TEST_TMPDIR/inventory.XXXXXX")"
    run_id="${run_dir##*.}"
    inventory_sessions=()
    pty_records=()
    test_logs=()
}

teardown() {
    local record pid transcript input log
    for record in "${pty_records[@]:-}"; do
        IFS=$'\t' read -r pid transcript input <<<"$record"
        PTY_PID="$pid"
        PTY_TRANSCRIPT="$transcript"
        PTY_INPUT="$input"
        pty_force_cleanup
    done

    "$ACCEPTANCE_CLEANUP" "$STAY_BIN" "$TMUX_TMPDIR" "${inventory_sessions[@]:-}" \
        >/dev/null 2>&1 || :
    for log in "${test_logs[@]:-}"; do
        rm -f -- "$log"
    done
    rmdir "$run_dir" 2>/dev/null || :
}

assert_empty_inventory() {
    run stay list
    [ "$status" -eq 0 ]
    [ -z "$output" ]

    run stay list --json
    [ "$status" -eq 0 ]
    [ "$output" = '{"sessions":[]}' ]
}

wait_for_terminated() {
    local session="$1" expected="$2" fragment output attempt
    fragment="\"name\":\"$session\",\"status\":\"terminated\""
    sleep 1
    for attempt in {1..100}; do
        : "$attempt"
        if output="$("$STAY_BIN" list --json 2>/dev/null)"; then
            if [[ "$output" == *"$fragment"* ]] && [[ "$output" == *"$expected"* ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    echo "timed out waiting for $session termination ($expected)" >&2
    "$STAY_BIN" list --json >&2 || :
    return 1
}

setup_inventory_fixture() {
    local detached_one="inventory-${run_id}-detached-1"
    local detached_two="inventory-${run_id}-detached-2"
    local attached_one="inventory-${run_id}-attached-1"
    local attached_two="inventory-${run_id}-attached-2"
    local terminated_one="inventory-${run_id}-terminated-1"
    local terminated_two="inventory-${run_id}-terminated-2"
    inventory_sessions=(
        "$detached_one"
        "$detached_two"
        "$attached_one"
        "$attached_two"
        "$terminated_one"
        "$terminated_two"
    )

    run stay create "$detached_one" -- sleep 60
    [ "$status" -eq 0 ]
    sleep 1
    run stay create "$detached_two" -- sleep 60
    [ "$status" -eq 0 ]
    sleep 1

    run stay create "$attached_one" -- sleep 60
    [ "$status" -eq 0 ]
    sleep 1
    pty_start "$STAY_BIN" attach "$attached_one"
    pty_records+=("$PTY_PID"$'\t'"$PTY_TRANSCRIPT"$'\t'"$PTY_INPUT")
    pty_wait_until_attached "$attached_one"

    run stay create "$attached_two" -- sleep 60
    [ "$status" -eq 0 ]
    sleep 1
    pty_start "$STAY_BIN" attach "$attached_two"
    pty_records+=("$PTY_PID"$'\t'"$PTY_TRANSCRIPT"$'\t'"$PTY_INPUT")
    pty_wait_until_attached "$attached_two"

    run stay create "$terminated_one" -- sh -c 'exit 7'
    [ "$status" -eq 0 ]
    wait_for_terminated "$terminated_one" '"exit_code":7'

    run stay create "$terminated_two" -- sh -c 'kill -TERM $$'
    [ "$status" -eq 0 ]
    wait_for_terminated "$terminated_two" '"signal":15'

}

assert_inventory_names() {
    local name
    for name in "${inventory_sessions[@]}"; do
        [[ "$output" == *"$name"* ]]
    done
}

assert_json_inventory() {
    local payload object name status
    local -a objects=()
    [[ "$output" != *$'\e['* ]]
    [[ "$output" == '{"sessions":['* ]]
    [[ "$output" == *']}' ]]

    local prefix='{"sessions":[' suffix=']}'
    payload="${output%$'\n'}"
    payload="${payload#"$prefix"}"
    payload="${payload%"$suffix"}"
    while IFS= read -r object; do
        [[ -n "$object" ]] && objects+=("$object")
    done < <(printf '%s\n' "$payload" | sed 's/},{/}\n{/g')
    [ "${#objects[@]}" -eq 6 ]

    local expected_names actual_names
    expected_names="$(printf '%s\n' "${inventory_sessions[@]}")"
    actual_names="$(printf '%s\n' "${objects[@]}" | sed -n 's/^{"name":"\([^"]*\)".*/\1/p')"
    [ "$actual_names" = "$expected_names" ]

    local i object_re
    for i in "${!inventory_sessions[@]}"; do
        object="${objects[$i]}"
        name="${inventory_sessions[$i]}"
        case "$i" in
            0 | 1)
                status=detached
                ;;
            2 | 3)
                status=attached
                ;;
            4)
                status=terminated
                ;;
            5)
                status=terminated
                ;;
        esac
        case "$i" in
            0 | 1 | 2 | 3)
                object_re="^\{\"name\":\"${name}\",\"status\":\"${status}\",\"created_at\":\"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\",\"current_directory\":\"[^\"]+\",\"current_command\":\"sleep\",\"terminated_at\":null,\"exit_code\":null,\"signal\":null\}$"
                ;;
            4)
                object_re="^\{\"name\":\"${name}\",\"status\":\"${status}\",\"created_at\":\"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\",\"current_directory\":null,\"current_command\":\"sh\",\"terminated_at\":\"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\",\"exit_code\":7,\"signal\":null\}$"
                ;;
            5)
                object_re="^\{\"name\":\"${name}\",\"status\":\"${status}\",\"created_at\":\"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\",\"current_directory\":null,\"current_command\":\"sh\",\"terminated_at\":\"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\",\"exit_code\":null,\"signal\":15\}$"
                ;;
        esac
        [[ "$object" =~ $object_re ]]
    done
}

@test "stay list shows the session inventory as human-readable rows" {
    assert_empty_inventory
    setup_inventory_fixture

    run stay list
    [ "$status" -eq 0 ]
    [[ "$output" != *$'\e['* ]]
    assert_inventory_names
    grep -Eq "^${inventory_sessions[0]}[[:space:]]+\[detached\]$" <<<"$output"
    grep -Eq "^${inventory_sessions[1]}[[:space:]]+\[detached\]$" <<<"$output"
    grep -Eq "^${inventory_sessions[2]}[[:space:]]+\[attached\]$" <<<"$output"
    grep -Eq "^${inventory_sessions[3]}[[:space:]]+\[attached\]$" <<<"$output"
    grep -Eq "^${inventory_sessions[4]}[[:space:]]+\[terminated exit=7 @[0-9]{4}-[0-9]{2}-[0-9]{2}T.*Z\]$" <<<"$output"
    grep -Eq "^${inventory_sessions[5]}[[:space:]]+\[terminated signal=15 @[0-9]{4}-[0-9]{2}-[0-9]{2}T.*Z\]$" <<<"$output"
}

@test "stay list --json emits a stable machine-readable inventory" {
    assert_empty_inventory
    setup_inventory_fixture

    run stay list --json
    [ "$status" -eq 0 ]
    assert_json_inventory
}
