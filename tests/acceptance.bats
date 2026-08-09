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

register_sessions() {
    inventory_sessions+=("$@")
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

wait_for_file_content() {
    local file="$1" expected="$2" actual attempt
    for attempt in {1..100}; do
        : "$attempt"
        if [[ -f "$file" ]]; then
            actual="$(cat "$file")"
            if [[ "$actual" == "$expected" ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    echo "timed out waiting for $file to contain expected content" >&2
    cat "$file" >&2 2>/dev/null || :
    return 1
}

wait_for_file_size() {
    local file="$1" expected="$2" actual attempt
    for attempt in {1..100}; do
        : "$attempt"
        if [[ -f "$file" ]]; then
            actual="$(wc -c <"$file" | tr -d '[:space:]')"
            if [[ "$actual" -eq "$expected" ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    echo "timed out waiting for $file to reach $expected bytes" >&2
    wc -c "$file" >&2 2>/dev/null || :
    return 1
}

assert_usage_error() {
    [ "$status" -eq 2 ]
    [ -z "$output" ]
    [[ "$stderr" == *"For more information, try '--help'."* ]]
    [ -n "$stderr" ]
}

@test "stay create uses the configured default command" {
    local session="lifecycle-${run_id}-default"
    register_sessions "$session"
    export STAY_CMD="sleep 60"

    run stay create "$session"
    [ "$status" -eq 0 ]

    run stay list
    [ "$status" -eq 0 ]
    [ "$output" = "$session [detached]" ]

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\",\"status\":\"detached\""* ]]

    run stay kill "$session"
    [ "$status" -eq 0 ]
}

@test "stay create preserves the command and its arguments" {
    local session="lifecycle-${run_id}-arguments"
    local args_file="$BATS_TEST_TMPDIR/arguments.txt"
    local expected=$'-leading\ntwo words'
    register_sessions "$session"

    # shellcheck disable=SC2016
    run stay create "$session" -- sh -c 'target=$1; shift; printf "%s\\n" "$@" >"$target"; sleep 60' \
        sh "$args_file" "-leading" "two words"
    [ "$status" -eq 0 ]
    wait_for_file_content "$args_file" "$expected"

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\""* ]]
}

@test "stay create starts the session in the requested directory" {
    local session="lifecycle-${run_id}-cwd"
    local cwd="$BATS_TEST_TMPDIR/working-directory"
    register_sessions "$session"
    mkdir -p "$cwd"
    cwd="$(cd "$cwd" && pwd -P)"

    run stay create "$session" --cwd "$cwd" -- sleep 60
    [ "$status" -eq 0 ]
    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\""* ]]
    [[ "$output" == *"\"current_directory\":\"$cwd\""* ]]
}

@test "stay create --force-recreate replaces an existing session" {
    local live="lifecycle-${run_id}-force-live"
    local terminated="lifecycle-${run_id}-force-terminated"
    register_sessions "$live" "$terminated"

    run stay create "$live" -- sleep 60
    [ "$status" -eq 0 ]
    run stay create "$live" --force-recreate -- sh -c 'sleep 60'
    [ "$status" -eq 0 ]
    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$live\",\"status\":\"detached\""* ]]

    run stay create "$terminated" -- sh -c 'exit 7'
    [ "$status" -eq 0 ]
    wait_for_terminated "$terminated" '"exit_code":7'
    run --separate-stderr stay create "$terminated" --force-recreate -- sleep 60
    [ "$status" -eq 0 ]
    [[ "$stderr" == *"terminated with exit code 7"* ]]
    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$terminated\",\"status\":\"detached\""* ]]
    [[ "$output" == *'"current_command":"sleep"'* ]]
}

@test "stay create rejects duplicates and invalid session names" {
    local session="lifecycle-${run_id}-duplicate"
    local long_name
    register_sessions "$session"
    long_name="$(printf 'a%.0s' {1..129})"

    run stay create "$session" -- sleep 60
    [ "$status" -eq 0 ]
    run --separate-stderr stay create "$session" -- sleep 60
    [ "$status" -eq 1 ]
    [ -z "$output" ]
    [[ "$stderr" == *"already exists"* ]]

    run --separate-stderr stay create "${session}.bad" -- sleep 60
    assert_usage_error
    [[ "$stderr" == *"disallowed character '.'"* ]]

    run --separate-stderr stay create "$long_name" -- sleep 60
    assert_usage_error
    [[ "$stderr" == *"must not exceed 128 Unicode characters"* ]]

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\""* ]]
    [[ "$output" != *"${session}.bad"* ]]
}

@test "stay kill removes live and terminated sessions" {
    local live="lifecycle-${run_id}-kill-live"
    local terminated="lifecycle-${run_id}-kill-terminated"
    register_sessions "$live" "$terminated"

    run stay create "$live" -- sleep 60
    [ "$status" -eq 0 ]
    run stay create "$terminated" -- sh -c 'exit 7'
    [ "$status" -eq 0 ]
    wait_for_terminated "$terminated" '"exit_code":7'

    run stay kill "$live"
    [ "$status" -eq 0 ]
    run stay kill "$terminated"
    [ "$status" -eq 0 ]
    assert_empty_inventory
}

@test "stay kill reports missing and invalid sessions" {
    local keeper="lifecycle-${run_id}-kill-keeper"
    register_sessions "$keeper"
    run stay create "$keeper" -- sleep 60
    [ "$status" -eq 0 ]

    run --separate-stderr stay kill "lifecycle-${run_id}-missing"
    [ "$status" -eq 1 ]
    [ -z "$output" ]
    [[ "$stderr" == *"can't find session"* ]]

    run --separate-stderr stay kill "lifecycle-${run_id}.bad"
    assert_usage_error
    [[ "$stderr" == *"disallowed character '.'"* ]]
}

@test "stay help lists commands and options" {
    run --separate-stderr stay --help
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    for command in list create attach kill shell-integration; do
        [[ "$output" == *"$command"* ]]
    done
    [[ "$output" == *"--prompt-integration"* ]]
    [[ "$output" == *"--no-alt-screen"* ]]

    run --separate-stderr stay list --help
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [[ "$output" == *"--json"* ]]

    run --separate-stderr stay create --help
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    for option in "-c, --cwd" "-f, --force-recreate" "-a, --attach" \
        "-r, --read-only" "-L, --low-priority"; do
        [[ "$output" == *"$option"* ]]
    done

    run --separate-stderr stay attach --help
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    for option in "-l, --log" "-t, --truncate" "--raw" \
        "-r, --read-only" "-L, --low-priority" "-p, --pass-through"; do
        [[ "$output" == *"$option"* ]]
    done

    run --separate-stderr stay kill --help
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [[ "$output" == *"SESSION"* ]]

    run --separate-stderr stay shell-integration --help
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [[ "$output" == *"--s-alias"* ]]
}

@test "stay version prints the package version" {
    run --separate-stderr stay --version
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [ "$output" = "stay 0.0.78" ]
}

@test "stay rejects invalid arguments and session names" {
    local long_name
    long_name="$(printf '界%.0s' {1..129})"
    for args in "bogus" "list --bogus" "create" "attach" "kill"; do
        # shellcheck disable=SC2086
        run --separate-stderr stay $args
        assert_usage_error
        [[ "$stderr" == *"Usage:"* ]]
    done

    run --separate-stderr stay create "${run_id}.bad"
    assert_usage_error
    [[ "$stderr" == *"disallowed character '.'"* ]]

    run --separate-stderr stay create "$long_name"
    assert_usage_error
    [[ "$stderr" == *"must not exceed 128 Unicode characters"* ]]
}

@test "stay rejects conflicting options" {
    for option in --read-only --low-priority; do
        run --separate-stderr stay create "lifecycle-${run_id}-conflict" "$option"
        assert_usage_error
        [[ "$stderr" == *"require -a/--attach"* ]]
    done

    for option in --truncate --raw; do
        run --separate-stderr stay attach "lifecycle-${run_id}-conflict" "$option"
        assert_usage_error
        [[ "$stderr" == *"requires -l/--log"* ]]
    done

    for args in \
        "attach lifecycle-${run_id}-conflict --pass-through --read-only" \
        "attach lifecycle-${run_id}-conflict --pass-through --low-priority" \
        "attach lifecycle-${run_id}-conflict --pass-through --log FILE"; do
        # shellcheck disable=SC2086
        run --separate-stderr stay $args
        assert_usage_error
        [[ "$stderr" == *"conflicts"* ]]
    done

    run --separate-stderr stay list --no-alt-screen
    assert_usage_error
    [[ "$stderr" == *"only applies"* ]]
    run --separate-stderr stay --prompt-integration list
    assert_usage_error
    [[ "$stderr" == *"mutually exclusive"* ]]
    run --separate-stderr stay --prompt-integration --no-alt-screen
    assert_usage_error
    [[ "$stderr" == *"mutually exclusive"* ]]
}

@test "stay attach --pass-through forwards stdin without attaching" {
    local incremental="lifecycle-${run_id}-incremental"
    local partial="lifecycle-${run_id}-partial"
    local large="lifecycle-${run_id}-large"
    local incremental_file="$BATS_TEST_TMPDIR/incremental.txt"
    local partial_file="$BATS_TEST_TMPDIR/partial.txt"
    local large_file="$BATS_TEST_TMPDIR/large.txt"
    register_sessions "$incremental" "$partial" "$large"

    # shellcheck disable=SC2016
    local capture_command='stty -icanon -echo; cat >"$1"; sleep 60'
    run stay create "$incremental" -- sh -c "$capture_command" sh "$incremental_file"
    [ "$status" -eq 0 ]
    run bash -c '{ printf first; sleep 0.2; printf "second\\n"; } | "$STAY_BIN" attach "$1" --pass-through' _ "$incremental"
    [ "$status" -eq 0 ]
    wait_for_file_content "$incremental_file" firstsecond

    run stay create "$partial" -- sh -c "$capture_command" sh "$partial_file"
    [ "$status" -eq 0 ]
    run bash -c 'printf partial | "$STAY_BIN" attach "$1" --pass-through' _ "$partial"
    [ "$status" -eq 0 ]
    wait_for_file_content "$partial_file" partial

    run stay create "$large" -- sh -c "$capture_command" sh "$large_file"
    [ "$status" -eq 0 ]
    run bash -c 'head -c 20000 /dev/zero | tr "\\0" X | "$STAY_BIN" attach "$1" --pass-through' _ "$large"
    [ "$status" -eq 0 ]
    wait_for_file_size "$large_file" 20000
    [ "$(head -c 8 "$large_file")" = "XXXXXXXX" ]
    [ "$(tail -c 8 "$large_file")" = "XXXXXXXX" ]
}

@test "stay enforces its tmux environment boundary" {
    local session="lifecycle-${run_id}-boundary"
    local fakebin="$BATS_TEST_TMPDIR/fakebin"
    local fake_tmux="$fakebin/tmux"
    register_sessions "$session"

    run stay create "$session" -- sleep 60
    [ "$status" -eq 0 ]
    for args in "list" "create lifecycle-${run_id}-blocked -- sleep 60" \
        "attach $session" "kill $session"; do
        # shellcheck disable=SC2086
        run --separate-stderr env TMUX=fake "$STAY_BIN" $args
        [ "$status" -eq 1 ]
        [ -z "$output" ]
        [[ "$stderr" == *"cannot run from inside tmux"* ]]
    done

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\""* ]]

    mkdir -p "$fakebin"
    printf '#!/bin/sh\nprintf "tmux 3.5\\n"\n' >"$fake_tmux"
    chmod +x "$fake_tmux"
    run --separate-stderr env PATH="$fakebin:$STAY_BIN_DIR" "$STAY_BIN" list
    [ "$status" -eq 1 ]
    [ -z "$output" ]
    [[ "$stderr" == *"tmux 3.6 or newer"* ]]

    run --separate-stderr env PATH="$STAY_BIN_DIR" "$STAY_BIN" list
    [ "$status" -eq 1 ]
    [ -z "$output" ]
    [[ "$stderr" == *"tmux is required but was not found on PATH"* ]]

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\""* ]]
    run stay kill "$session"
    [ "$status" -eq 0 ]
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
