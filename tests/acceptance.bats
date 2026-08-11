#!/usr/bin/env bats

bats_require_minimum_version 1.14.0

stay() {
    "$STAY_BIN" "$@"
}

setup_file() {
    load helpers/acceptance_pty.bash
    load helpers/acceptance_tmux.bash
    local helper_file="$BATS_FILE_TMPDIR/acceptance-helpers.bash"
    {
        declare -f \
            pty_start \
            pty_wait_until_attached \
            pty_send_input \
            pty_send_detach \
            pty_wait \
            pty_force_cleanup \
            _pty_wait_until_detached \
            _pty_wait_until_output \
            _pty_assert_output_absent \
            _pty_wait_until_exit \
            _pty_wait_reap \
            acceptance_tmux_wait_until_output \
            acceptance_tmux_wait_until_client_flag \
            _acceptance_tmux_validate_socket_root
    } >"$helper_file"
    export BASH_ENV="$helper_file"

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

wait_for_file_nonempty() {
    local file="$1" actual attempt
    for attempt in {1..100}; do
        : "$attempt"
        if [[ -f "$file" ]]; then
            actual="$(cat "$file")"
            if [[ -n "$actual" ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    echo "timed out waiting for $file to become nonempty" >&2
    cat "$file" >&2 2>/dev/null || :
    return 1
}

wait_for_process_gone() {
    local pid="$1" attempt
    for attempt in {1..100}; do
        : "$attempt"
        if ! kill -0 "$pid" 2>/dev/null; then
            return 0
        fi
        sleep 0.1
    done
    echo "timed out waiting for process $pid to exit" >&2
    ps -p "$pid" -o pid=,stat=,command= >&2 2>/dev/null || :
    return 1
}

wait_for_file_contains() {
    local file="$1" marker="$2" attempt
    for attempt in {1..100}; do
        : "$attempt"
        if [[ -f "$file" ]] && grep -Fq -- "$marker" "$file"; then
            return 0
        fi
        sleep 0.1
    done
    echo "timed out waiting for $file to contain $marker" >&2
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

register_pty() {
    pty_records+=("$PTY_PID"$'\t'"$PTY_TRANSCRIPT"$'\t'"$PTY_INPUT")
}

register_log() {
    local log_path
    for log_path in "$@"; do
        test_logs+=("$log_path" "$log_path.offset" "$log_path.offset.tmp")
    done
}

log_mode() {
    case "$(uname -s)" in
        Darwin) stat -f '%Lp' "$1" ;;
        Linux) stat -c '%a' "$1" ;;
        *) return 1 ;;
    esac
}

count_log_line() {
    local log_path="$1" marker="$2"
    sed 's/[[:space:]]*$//' "$log_path" | grep -Fxc -- "$marker" || :
}

wait_for_pty_status() {
    local expected="$1" actual record remaining existing
    record="$PTY_PID"$'\t'"$PTY_TRANSCRIPT"$'\t'"$PTY_INPUT"
    pty_wait --exit
    if pty_wait; then
        actual=0
    else
        actual=$?
    fi
    remaining=()
    for existing in "${pty_records[@]}"; do
        [[ "$existing" != "$record" ]] && remaining+=("$existing")
    done
    pty_records=("${remaining[@]}")
    if [ "$actual" -ne "$expected" ]; then
        echo "unexpected PTY status: expected $expected, got $actual" >&2
        return 1
    fi
}

@test "stay create uses the configured default command" {
    local session="lifecycle-${run_id}-default"
    local marker="$BATS_TEST_TMPDIR/$session.pid"
    register_sessions "$session"
    export STAY_CMD="printf '%s\\n' \"\$\$\" >\"$marker\"; exec sleep 60"

    run stay create "$session"
    [ "$status" -eq 0 ]
    wait_for_file_nonempty "$marker"

    run stay list
    [ "$status" -eq 0 ]
    [ "$output" = "$session [detached]" ]

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\",\"status\":\"detached\""* ]]
    [[ "$output" == *"\"name\":\"$session\""*"\"current_command\":\"sleep\""* ]]

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
    local marker="$BATS_TEST_TMPDIR/$session.pwd"
    register_sessions "$session"
    mkdir -p "$cwd"
    cwd="$(cd "$cwd" && pwd -P)"

    # shellcheck disable=SC2016
    run stay create "$session" --cwd "$cwd" -- sh -c 'pwd >"$1"; exec sleep 60' sh "$marker"
    [ "$status" -eq 0 ]
    wait_for_file_content "$marker" "$cwd"
    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$session\""* ]]
    [[ "$output" == *"\"current_directory\":\"$cwd\""* ]]
}

@test "stay create --force-recreate replaces an existing session" {
    local live="lifecycle-${run_id}-force-live"
    local terminated="lifecycle-${run_id}-force-terminated"
    local old_pid_file="$BATS_TEST_TMPDIR/$live.old.pid"
    local new_pid_file="$BATS_TEST_TMPDIR/$live.new.pid"
    local old_pid new_pid
    register_sessions "$live" "$terminated"

    # shellcheck disable=SC2016
    run stay create "$live" -- sh -c 'printf "%s\\n" "$$" >"$1"; exec sleep 60' sh "$old_pid_file"
    [ "$status" -eq 0 ]
    wait_for_file_nonempty "$old_pid_file"
    old_pid="$(cat "$old_pid_file")"
    [[ "$old_pid" =~ ^[0-9]+$ ]]

    # shellcheck disable=SC2016
    run stay create "$live" --force-recreate -- sh -c 'printf "%s\\n" "$$" >"$1"; exec sleep 60' sh "$new_pid_file"
    [ "$status" -eq 0 ]
    wait_for_file_nonempty "$new_pid_file"
    new_pid="$(cat "$new_pid_file")"
    [[ "$new_pid" =~ ^[0-9]+$ ]]
    [ "$old_pid" -ne "$new_pid" ]
    wait_for_process_gone "$old_pid"
    kill -0 "$new_pid"

    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$live\",\"status\":\"detached\""* ]]
    [[ "$output" == *"\"name\":\"$live\""*"\"current_command\":\"sleep\""* ]]

    run stay create "$terminated" -- sh -c 'exit 7'
    [ "$status" -eq 0 ]
    wait_for_terminated "$terminated" '"exit_code":7'
    run --separate-stderr stay create "$terminated" --force-recreate -- sleep 60
    [ "$status" -eq 0 ]
    [[ "$stderr" == *"terminated with exit code 7"* ]]
    run stay list --json
    [ "$status" -eq 0 ]
    [[ "$output" == *"\"name\":\"$terminated\",\"status\":\"detached\""* ]]
    [[ "$output" == *"\"name\":\"$terminated\""*"\"current_command\":\"sleep\""* ]]
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
    local package_version
    package_version="$(
        sed -n 's/^version = "\([^"]*\)"/\1/p' \
            "$BATS_TEST_DIRNAME/../Cargo.toml" | head -n 1
    )"
    run --separate-stderr stay --version
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [ "$output" = "stay $package_version" ]
}

@test "stay create --attach creates and attaches a session" {
    local session="relay-create-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    # shellcheck disable=SC2016
    local fixture='printf ready; read value; printf "value=%s\\n" "$value"; exit 7'
    register_sessions "$session"

    pty_start "$STAY_BIN" create "$session" --attach -- sh -c "$fixture"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"

    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_input $'input\n'
    pty_wait --output "value=input"
    wait_for_pty_status 7
    wait_for_file_contains "$log_path" "value=input"
    wait_for_terminated "$session" '"exit_code":7'
}

@test "stay attach --log captures clean output across attaches" {
    local session="logging-clean-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    # shellcheck disable=SC2016
    local fixture='printf "retained-marker\nready\n"; read go; printf "periodic-marker\n"; i=0; while [ "$i" -lt 40 ]; do printf "filler-%02d\n" "$i"; i=$((i+1)); done; printf "visible-marker\n"; sleep 30'
    register_sessions "$session"
    register_log "$log_path"

    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    pty_send_input $'go\n'
    wait_for_file_contains "$log_path" periodic-marker
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    wait_for_file_contains "$log_path" visible-marker

    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"

    local contents
    contents="$(cat "$log_path")"
    [ "$(count_log_line "$log_path" retained-marker)" -eq 1 ]
    [ "$(count_log_line "$log_path" periodic-marker)" -eq 1 ]
    [ "$(count_log_line "$log_path" visible-marker)" -eq 1 ]
    if LC_ALL=C grep -Fq $'\033' "$log_path"; then
        false
    fi
    [ "$(log_mode "$log_path")" = 600 ]
    [[ "$contents" == *retained-marker* ]]
}

@test "stay attach --log --truncate overwrites the log" {
    local session="logging-truncate-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    # shellcheck disable=SC2016
    local fixture='printf "fresh-marker\n"; sleep 30'
    register_sessions "$session"
    register_log "$log_path"
    printf 'stale-before\n' >"$log_path"
    chmod 600 "$log_path"

    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$session" --log "$log_path" --truncate
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output fresh-marker
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    wait_for_file_contains "$log_path" fresh-marker
    run grep -Fq stale-before "$log_path"
    [ "$status" -eq 1 ]

    printf 'stale-between\n' >>"$log_path"
    chmod 600 "$log_path"
    pty_start "$STAY_BIN" attach "$session" --log "$log_path" --truncate
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ "$(count_log_line "$log_path" fresh-marker)" -eq 1 ]
    run grep -Fq stale-between "$log_path"
    [ "$status" -eq 1 ]
}

@test "stay attach --log --raw preserves ANSI and streams output" {
    local session="logging-raw-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    # shellcheck disable=SC2016
    local fixture='printf "\033[31mraw-start\033[0m\n"; i=0; while [ "$i" -lt 100 ]; do printf "\033[32mraw-tick-%03d\033[0m\n" "$i"; i=$((i+1)); sleep .05; done; sleep 30'
    register_sessions "$session"
    register_log "$log_path"

    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$session" --log "$log_path" --raw
    register_pty
    pty_wait_until_attached "$session"
    wait_for_file_contains "$log_path" raw-start
    if ! LC_ALL=C grep -Fq $'\033[31m' "$log_path"; then
        false
    fi
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    wait_for_file_contains "$log_path" raw-tick-020
    local size_at_second_attach
    size_at_second_attach="$(wc -c <"$log_path" | tr -d '[:space:]')"

    pty_start "$STAY_BIN" attach "$session" --log "$log_path" --raw
    register_pty
    pty_wait_until_attached "$session"
    wait_for_file_contains "$log_path" raw-tick-040
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ "$(wc -c <"$log_path" | tr -d '[:space:]')" -gt "$size_at_second_attach" ]
}

@test "stay logging handles history and capture boundaries" {
    local session="logging-boundary-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    local sidecar="$log_path.offset"
    # shellcheck disable=SC2016
    local fixture='i=0; while [ "$i" -lt 3000 ]; do printf "large-%04d.................................................................\n" "$i"; i=$((i+1)); done; printf "visible-boundary\n"; sleep 30'
    register_sessions "$session"
    register_log "$log_path"

    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output visible-boundary
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    wait_for_file_contains "$log_path" large-2999
    [ "$(wc -c <"$log_path" | tr -d '[:space:]')" -gt 65536 ]
    grep -Fqx visible-boundary "$log_path"

    rm -f -- "$sidecar"
    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ -f "$sidecar" ]

    printf 'not-a-cursor\n' >"$sidecar"
    chmod 600 "$sidecar"
    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    grep -Fq -- '--- history evicted before capture ---' "$log_path"

    printf 'session=other\nlog_size=1\nline_count=1\npartial=0\nmarker_bytes=0\nanchor=6f6c640a\n' >"$sidecar"
    chmod 600 "$sidecar"
    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ -f "$sidecar" ]
}

@test "stay logging preserves output across repeated history boundaries" {
    local session="logging-stress-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    export STAY_HISTORY_LINES=600
    export STAY_LOG_CAPTURE_INTERVAL_SECONDS=1
    # shellcheck disable=SC2016
    local fixture='printf "ready\n"; read go; batch=0; while [ "$batch" -lt 6 ]; do i=0; while [ "$i" -lt 80 ]; do printf "paced-%d-%02d\n" "$batch" "$i"; i=$((i+1)); done; batch=$((batch+1)); sleep 1.5; done; i=0; while [ "$i" -lt 40 ]; do printf "settle-%02d\n" "$i"; i=$((i+1)); done; sleep 5; i=0; while [ "$i" -lt 1000 ]; do printf "flood-%04d\n" "$i"; i=$((i+1)); done; printf "flood-final\n"; i=0; while [ "$i" -lt 40 ]; do printf "flood-settle-%02d\n" "$i"; i=$((i+1)); done; sleep 30'
    register_sessions "$session"
    register_log "$log_path"

    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$session" --log "$log_path" --truncate
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    pty_send_input $'go\n'
    wait_for_file_contains "$log_path" paced-0-00
    wait_for_file_contains "$log_path" paced-5-79
    acceptance_tmux_wait_until_output "$session" flood-final
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"

    local batch index marker
    for batch in {0..5}; do
        for index in {00..79}; do
            marker="paced-$batch-$index"
            [ "$(count_log_line "$log_path" "$marker")" -eq 1 ]
        done
    done
    [ "$(count_log_line "$log_path" '--- history evicted before capture ---')" -ge 1 ]
    [ "$(count_log_line "$log_path" flood-0999)" -eq 1 ]
    [ "$(count_log_line "$log_path" flood-final)" -eq 1 ]
    for index in {00..39}; do
        marker="flood-settle-$index"
        [ "$(count_log_line "$log_path" "$marker")" -eq 1 ]
    done
}

@test "stay logging rejects unsafe log targets" {
    local session="logging-unsafe-${run_id}"
    local cwd="$BATS_TEST_TMPDIR/client-cwd"
    local relative_log="$cwd/relative.log"
    local symlink_path="$BATS_TEST_TMPDIR/log-link"
    local sentinel="$BATS_TEST_TMPDIR/log-sentinel"
    local directory_path="$BATS_TEST_TMPDIR/log-directory"
    local open_path="$BATS_TEST_TMPDIR/log-open"
    local sidecar_path="$BATS_TEST_TMPDIR/log-sidecar"
    local temp_path="$BATS_TEST_TMPDIR/log-temp"
    register_sessions "$session"
    register_log "$relative_log" "$symlink_path" "$sentinel" "$directory_path" "$open_path" "$sidecar_path" "$temp_path"
    mkdir -p "$cwd" "$directory_path"
    printf untouched >"$sentinel"
    ln -s "$sentinel" "$symlink_path"
    printf '' >"$open_path"
    chmod 644 "$open_path"

    run stay create "$session" -- sleep 30
    [ "$status" -eq 0 ]
    # shellcheck disable=SC2016
    pty_start sh -c 'cd "$1"; exec "$2" attach "$3" --log relative.log' sh "$cwd" "$STAY_BIN" "$session"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ -f "$relative_log" ]
    [ ! -e "$BATS_TEST_DIRNAME/relative.log" ]
    [ "$(log_mode "$relative_log")" = 600 ]

    run --separate-stderr stay attach "$session" --log "$symlink_path"
    [ "$status" -eq 1 ]
    [[ "$stderr" == *symlink* ]]
    [ "$(cat "$sentinel")" = untouched ]

    run --separate-stderr stay attach "$session" --log "$directory_path"
    [ "$status" -eq 1 ]
    [[ "$stderr" == *"not a regular file"* ]]
    run --separate-stderr stay attach "$session" --log "$open_path"
    [ "$status" -eq 1 ]
    [[ "$stderr" == *"group or other"* ]]

    local safe_log="$BATS_TEST_TMPDIR/log-cursor"
    register_log "$safe_log"
    printf '' >"$safe_log"
    chmod 600 "$safe_log"
    ln -s "$sentinel" "$sidecar_path"
    pty_start "$STAY_BIN" attach "$session" --log "$safe_log"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ "$(cat "$sentinel")" = untouched ]
    rm -f -- "$sidecar_path"
    ln -s "$sentinel" "$temp_path"
    pty_start "$STAY_BIN" attach "$session" --log "$safe_log"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    [ "$(cat "$sentinel")" = untouched ]
}

@test "stay logging survives target failures safely" {
    local session="logging-failure-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    # shellcheck disable=SC2016
    local fixture='printf "before-failure\n"; i=0; while [ "$i" -lt 30 ]; do printf "during-failure-%02d\n" "$i"; i=$((i+1)); sleep .2; done; printf "final-after-failure\n"; sleep 30'
    register_sessions "$session"
    register_log "$log_path"
    printf '' >"$log_path"
    chmod 600 "$log_path"

    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$session" --log "$log_path"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output before-failure
    rm -f -- "$log_path"
    mkdir "$log_path"
    pty_wait --output during-failure-05
    pty_wait --output "failed to write log"
    [ "$(grep -Fo 'failed to write log' "$PTY_TRANSCRIPT" | wc -l | tr -d '[:space:]')" -eq 1 ]
    rmdir "$log_path"
    printf '' >"$log_path"
    chmod 600 "$log_path"
    pty_wait --output final-after-failure
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
    grep -Fqx final-after-failure "$log_path"
    [ -f "$log_path.offset" ]
    [ "$(log_mode "$log_path.offset")" = 600 ]
}

@test "stay logging validates its option combinations" {
    local session="logging-options-${run_id}"
    local log_path="$BATS_TEST_TMPDIR/$session.log"
    register_sessions "$session"
    register_log "$log_path"

    for option in --truncate --raw; do
        run --separate-stderr stay attach "$session" "$option"
        assert_usage_error
        [[ "$stderr" == *"requires -l/--log"* ]]
    done
    for args in \
        "attach $session --pass-through --log $log_path" \
        "attach $session --pass-through --raw --log $log_path"; do
        # shellcheck disable=SC2086
        run --separate-stderr stay $args
        assert_usage_error
        [[ "$stderr" == *conflicts* ]]
    done
    for args in \
        "create $session --log $log_path" \
        "create $session --truncate" \
        "create $session --raw"; do
        # shellcheck disable=SC2086
        run --separate-stderr stay $args
        assert_usage_error
    done
}

@test "stay create --attach --read-only prevents input changes" {
    local session="relay-create-read-only-${run_id}"
    # shellcheck disable=SC2016
    local fixture='printf ready; while IFS= read -r value; do test -n "$value" && printf "received=%s\\n" "$value"; done; sleep 30'
    register_sessions "$session"

    pty_start "$STAY_BIN" create "$session" --attach --read-only -- sh -c "$fixture"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    pty_send_input $'should-not-reach\n'
    pty_wait --absent "received="
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
}

@test "stay create --attach --low-priority attaches at low priority" {
    local session="relay-create-low-priority-${run_id}"
    # shellcheck disable=SC2016
    local fixture='printf ready; read value; printf "value=%s\\n" "$value"; sleep 30'
    register_sessions "$session"

    pty_start "$STAY_BIN" create "$session" --attach --low-priority -- sh -c "$fixture"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    acceptance_tmux_wait_until_client_flag "$session" ignore-size
    pty_send_input $'low-priority\n'
    pty_wait --output "value=low-priority"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
}

@test "stay attach relays input and output and detaches cleanly" {
    local session="relay-attach-${run_id}"
    # shellcheck disable=SC2016
    local fixture='printf ready; read value; printf "received=%s\\n" "$value"; sleep 30'
    register_sessions "$session"
    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]

    # shellcheck disable=SC2016
    pty_start sh -c '"$1" attach "$2"; stty -a' sh "$STAY_BIN" "$session"
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    pty_send_input $'input\n'
    pty_wait --output "received=input"
    pty_send_detach
    pty_wait --output icanon
    pty_wait --output echo
    wait_for_pty_status 0
    pty_wait --detached "$session"

    pty_start "$STAY_BIN" attach "$session"
    register_pty
    pty_wait_until_attached "$session"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
}

@test "stay attach --read-only prevents mutating input" {
    local session="relay-attach-read-only-${run_id}"
    # shellcheck disable=SC2016
    local fixture='printf ready; while IFS= read -r value; do test -n "$value" && printf "received=%s\\n" "$value"; done; sleep 30'
    register_sessions "$session"
    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]

    pty_start "$STAY_BIN" attach "$session" --read-only
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    pty_send_input $'should-not-reach\n'
    pty_wait --absent "received="
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
}

@test "stay attach --low-priority uses the low-priority client mode" {
    local session="relay-attach-low-priority-${run_id}"
    # shellcheck disable=SC2016
    local fixture='printf ready; read value; printf "value=%s\\n" "$value"; sleep 30'
    register_sessions "$session"
    run stay create "$session" -- sh -c "$fixture"
    [ "$status" -eq 0 ]

    pty_start "$STAY_BIN" attach "$session" --low-priority
    register_pty
    pty_wait_until_attached "$session"
    pty_wait --output ready
    acceptance_tmux_wait_until_client_flag "$session" ignore-size
    pty_send_input $'low-priority\n'
    pty_wait --output "value=low-priority"
    pty_send_detach
    wait_for_pty_status 0
    pty_wait --detached "$session"
}

@test "stay attach reports failures and preserves exit status" {
    local missing="relay-missing-${run_id}"
    run --separate-stderr stay attach "$missing"
    [ "$status" -eq 1 ]
    [ -z "$output" ]
    [[ "$stderr" == *"session \"$missing\" does not exist"* ]]

    local exited="relay-exit-${run_id}"
    register_sessions "$exited"
    run stay create "$exited" -- sh -c 'sleep 5; exit 7'
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$exited"
    register_pty
    pty_wait_until_attached "$exited"
    wait_for_pty_status 7
    wait_for_terminated "$exited" '"exit_code":7'

    local signalled="relay-signal-${run_id}"
    local release="$BATS_TEST_TMPDIR/$signalled.release"
    register_sessions "$signalled"
    # shellcheck disable=SC2016
    local signal_fixture='while test ! -e "$1"; do sleep .01; done; kill -TERM $$'
    run stay create "$signalled" -- sh -c "$signal_fixture" sh "$release"
    [ "$status" -eq 0 ]
    pty_start "$STAY_BIN" attach "$signalled"
    register_pty
    pty_wait_until_attached "$signalled"
    run touch "$release"
    [ "$status" -eq 0 ]
    wait_for_pty_status 143
    wait_for_terminated "$signalled" '"signal":15'

    local signal session pid_file pid
    for signal in HUP INT TERM; do
        session="relay-external-${signal,,}-${run_id}"
        pid_file="$BATS_TEST_TMPDIR/$session.pid"
        register_sessions "$session"
        run stay create "$session" -- sleep 30
        [ "$status" -eq 0 ]
        # shellcheck disable=SC2016
        pty_start sh -c 'printf "%s\\n" "$$" >"$1"; exec "$2" attach "$3"' \
            sh "$pid_file" "$STAY_BIN" "$session"
        register_pty
        pty_wait_until_attached "$session"
        wait_for_file_nonempty "$pid_file"
        pid="$(cat "$pid_file")"
        run kill "-$signal" "$pid"
        [ "$status" -eq 0 ]
        wait_for_pty_status 0
        pty_wait --detached "$session"
    done
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

@test "stay --prompt-integration prints a usable prompt function" {
    local snippet="$BATS_TEST_TMPDIR/prompt-integration.sh"
    local shell

    run --separate-stderr stay --prompt-integration
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [[ "$output" == *"stay_prompt_segment()"* ]]
    [[ "$output" == *"setopt PROMPT_SUBST"* ]]
    printf '%s\n' "$output" >"$snippet"

    for shell in sh bash zsh; do
        # shellcheck disable=SC2016
        run --separate-stderr "$shell" -c '
            . "$1"
            printf "without=[%s]\n" "$(stay_prompt_segment)"
            STAY_SESSION_NAME=work
            printf "with=[%s]\n" "$(stay_prompt_segment)"
        ' shell "$snippet"
        [ "$status" -eq 0 ]
        [ -z "$stderr" ]
        [ "$output" = $'without=[]\nwith=[[work] ]' ]
    done
}

@test "stay shell-integration prints the prompt snippet" {
    local startup_file
    local -a startup_files=(.bashrc .zshrc .profile)
    local expected

    for startup_file in "${startup_files[@]}"; do
        printf 'sentinel-%s\n' "$startup_file" >"$HOME/$startup_file"
    done

    run --separate-stderr env -u TMUX PATH="$STAY_BIN_DIR" \
        "$STAY_BIN" --prompt-integration
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    expected="$output"

    run --separate-stderr env TMUX=simulated PATH="$STAY_BIN_DIR" \
        "$STAY_BIN" shell-integration
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [ "$output" = "$expected" ]

    for startup_file in "${startup_files[@]}"; do
        [ "$(cat "$HOME/$startup_file")" = "sentinel-$startup_file" ]
    done
}

@test "stay shell-integration --s-alias adds the safe alias" {
    local startup_file conflict_dir
    local -a startup_files=(.bashrc .zshrc .profile)
    local snippet
    snippet="$(stay --prompt-integration)"

    run --separate-stderr env -u TMUX PATH="$ACCEPTANCE_TOOL_PATH:$STAY_BIN_DIR" \
        "$STAY_BIN" shell-integration --s-alias
    [ "$status" -eq 0 ]
    [ -z "$stderr" ]
    [[ "$output" == "$snippet"$'\nalias s=stay' ]]
    for startup_file in "${startup_files[@]}"; do
        [ ! -e "$HOME/$startup_file" ]
    done

    for startup_file in "${startup_files[@]}"; do
        printf 'alias s=existing\n' >"$HOME/$startup_file"
        run --separate-stderr env -u TMUX \
            PATH="$ACCEPTANCE_TOOL_PATH:$STAY_BIN_DIR" \
            "$STAY_BIN" shell-integration --s-alias
        [ "$status" -eq 0 ]
        [ "$output" = "$snippet" ]
        [[ "$stderr" == "warning: an 's' alias in ~/.${startup_file#*.} already exists; skipping 'alias s=stay' — add it yourself if you want to override it" ]]
        [ "$(cat "$HOME/$startup_file")" = 'alias s=existing' ]
        rm -f -- "$HOME/$startup_file"
    done

    conflict_dir="$BATS_TEST_TMPDIR/path-conflict"
    mkdir -p "$conflict_dir"
    : >"$conflict_dir/s"
    chmod +x "$conflict_dir/s"
    run --separate-stderr env -u TMUX \
        PATH="$conflict_dir:$ACCEPTANCE_TOOL_PATH:$STAY_BIN_DIR" \
        "$STAY_BIN" shell-integration --s-alias
    [ "$status" -eq 0 ]
    [ "$output" = "$snippet" ]
    [[ "$stderr" == "warning: an 's' command on PATH already exists; skipping 'alias s=stay' — add it yourself if you want to override it" ]]

    mkdir "$HOME/.profile"
    run --separate-stderr env -u TMUX \
        PATH="$ACCEPTANCE_TOOL_PATH:$STAY_BIN_DIR" \
        "$STAY_BIN" shell-integration --s-alias
    [ "$status" -eq 0 ]
    [ "$output" = "$snippet" ]
    [[ "$stderr" == "warning: cannot inspect alias in ~/.profile; treating it as an existing 's' alias and skipping 'alias s=stay' — restore read access or add it yourself if you want to override it" ]]
    rmdir "$HOME/.profile"

    printf 'unreadable\n' >"$HOME/.profile"
    chmod 000 "$HOME/.profile"
    run --separate-stderr env -u TMUX \
        PATH="$ACCEPTANCE_TOOL_PATH:$STAY_BIN_DIR" \
        "$STAY_BIN" shell-integration --s-alias
    [ "$status" -eq 0 ]
    [ "$output" = "$snippet" ]
    [[ "$stderr" == "warning: cannot inspect alias in ~/.profile; treating it as an existing 's' alias and skipping 'alias s=stay' — restore read access or add it yourself if you want to override it" ]]
}
