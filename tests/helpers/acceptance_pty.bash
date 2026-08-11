# Portable PTY operations for the Bats acceptance suite.

pty_start() {
    local command_string
    local word
    command_string=""
    for word in "$@"; do
        printf -v command_string '%s %q' "$command_string" "$word"
    done
    command_string="${command_string# }"

    local pty_dir
    pty_dir="$(mktemp -d "$BATS_TEST_TMPDIR/pty.XXXXXX")"
    PTY_TRANSCRIPT="$pty_dir/transcript.log"
    PTY_INPUT="$pty_dir/input.fifo"
    mkfifo "$PTY_INPUT"

    case "$(uname -s)" in
        Darwin)
            (
                python3 - "$PTY_TRANSCRIPT" "$PTY_INPUT" "$@" <<'PY'
import os
import pty
import select
import signal
import subprocess
import sys

transcript, input_path, *command = sys.argv[1:]
master, slave = pty.openpty()
process = subprocess.Popen(
    ["script", "-qeF", transcript, *command],
    env={**os.environ, "TERM": "xterm"},
    stdin=slave,
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
    start_new_session=True,
)
os.close(slave)
input_fd = os.open(input_path, os.O_RDWR)


def terminate(_signum, _frame):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass


signal.signal(signal.SIGINT, terminate)
signal.signal(signal.SIGTERM, terminate)
try:
    while process.poll() is None:
        readable, _, _ = select.select([input_fd], [], [], 0.1)
        if readable:
            data = os.read(input_fd, 4096)
            if data:
                os.write(master, data)
finally:
    os.close(input_fd)
    os.close(master)
    sys.exit(process.wait())
PY
            ) &
            ;;
        Linux)
            (
                exec 3<>"$PTY_INPUT"
                exec env TERM=xterm script -qefc "$command_string" "$PTY_TRANSCRIPT" <&3 >/dev/null 2>&1
            ) &
            ;;
        *)
            echo "unsupported PTY host: $(uname -s)" >&2
            rm -f -- "$PTY_INPUT"
            return 1
            ;;
    esac
    PTY_PID=$!
}

pty_wait_until_attached() {
    local session="$1" output attempt
    for attempt in {1..100}; do
        if output="$("$STAY_BIN" list --json 2>/dev/null)"; then
            if [[ "$output" == *"\"name\":\"$session\",\"status\":\"attached\""* ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    echo "timed out waiting for PTY client to attach to $session" >&2
    echo "PTY transcript: $PTY_TRANSCRIPT" >&2
    sed -n '1,12p' "$PTY_TRANSCRIPT" >&2 2>/dev/null || :
    "$STAY_BIN" list --json >&2 || :
    return 1
}

_pty_wait_until_detached() {
    local session="$1" output attempt
    for attempt in {1..100}; do
        : "$attempt"
        if output="$("$STAY_BIN" list --json 2>/dev/null)"; then
            if [[ "$output" == *"\"name\":\"$session\",\"status\":\"detached\""* ]]; then
                return 0
            fi
        fi
        sleep 0.1
    done
    echo "timed out waiting for PTY client to detach from $session" >&2
    echo "PTY transcript: $PTY_TRANSCRIPT" >&2
    sed -n '1,12p' "$PTY_TRANSCRIPT" >&2 2>/dev/null || :
    "$STAY_BIN" list --json >&2 || :
    return 1
}

_pty_wait_until_output() {
    local marker="$1" attempt
    for attempt in {1..100}; do
        : "$attempt"
        if [[ -f "$PTY_TRANSCRIPT" ]] &&
            tail -n +2 "$PTY_TRANSCRIPT" | grep -Fq -- "$marker"; then
            return 0
        fi
        sleep 0.1
    done
    echo "timed out waiting for PTY output: $marker" >&2
    echo "PTY transcript: $PTY_TRANSCRIPT" >&2
    sed -n '1,40p' "$PTY_TRANSCRIPT" >&2 2>/dev/null || :
    return 1
}

_pty_assert_output_absent() {
    local marker="$1" attempts="${2:-20}" attempt
    for ((attempt = 1; attempt <= attempts; attempt++)); do
        : "$attempt"
        if [[ -f "$PTY_TRANSCRIPT" ]] &&
            tail -n +2 "$PTY_TRANSCRIPT" | grep -Fq -- "$marker"; then
            echo "unexpected PTY output: $marker" >&2
            sed -n '1,40p' "$PTY_TRANSCRIPT" >&2 2>/dev/null || :
            return 1
        fi
        sleep 0.1
    done
    return 0
}

_pty_wait_until_exit() {
    local state attempt
    for attempt in {1..100}; do
        : "$attempt"
        state="$(ps -p "$PTY_PID" -o stat= 2>/dev/null || :)"
        state="${state//[[:space:]]/}"
        if [[ -z "$state" || "$state" == Z* ]]; then
            return 0
        fi
        sleep 0.1
    done
    echo "timed out waiting for PTY process $PTY_PID to exit" >&2
    echo "PTY transcript: $PTY_TRANSCRIPT" >&2
    sed -n '1,40p' "$PTY_TRANSCRIPT" >&2 2>/dev/null || :
    return 1
}

pty_send_input() {
    printf '%s' "$1" >"$PTY_INPUT"
}

pty_send_detach() {
    printf '\034' >"$PTY_INPUT"
}

pty_wait() {
    case "${1:-}" in
        --output)
            (($# == 2)) || {
                echo "usage: pty_wait --output MARKER" >&2
                return 2
            }
            _pty_wait_until_output "$2"
            ;;
        --absent)
            local attempts=20
            if (($# == 4)) && [[ "$3" == --attempts ]] &&
                [[ "$4" =~ ^[1-9][0-9]*$ ]]; then
                attempts="$4"
            elif (($# != 2)); then
                echo "usage: pty_wait --absent MARKER [--attempts N]" >&2
                return 2
            fi
            _pty_assert_output_absent "$2" "$attempts"
            ;;
        --detached)
            (($# == 2)) || {
                echo "usage: pty_wait --detached SESSION" >&2
                return 2
            }
            _pty_wait_until_detached "$2"
            ;;
        --exit)
            (($# == 1)) || {
                echo "usage: pty_wait --exit" >&2
                return 2
            }
            _pty_wait_until_exit
            ;;
        "")
            _pty_wait_reap
            ;;
        *)
            echo "usage: pty_wait [--output MARKER|--absent MARKER|--detached SESSION|--exit]" >&2
            return 2
            ;;
    esac
}

pty_force_cleanup() {
    local pid="${PTY_PID:-}" attempt pty_dir="${PTY_INPUT%/*}"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || :
        for attempt in {1..20}; do
            : "$attempt"
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.1
        done
        if kill -0 "$pid" 2>/dev/null; then
            kill -KILL "$pid" 2>/dev/null || :
        fi
    fi
    if [[ -n "$pid" ]]; then
        wait "$pid" 2>/dev/null || :
    fi
    rm -f -- "${PTY_INPUT:-}" "${PTY_TRANSCRIPT:-}"
    rmdir "$pty_dir" 2>/dev/null || :
    PTY_PID=""
    PTY_INPUT=""
    PTY_TRANSCRIPT=""
}

_pty_wait_reap() {
    local status pty_dir
    pty_dir="${PTY_INPUT%/*}"
    if wait "$PTY_PID"; then
        status=0
    else
        status=$?
    fi
    rm -f -- "$PTY_INPUT" "$PTY_TRANSCRIPT"
    rmdir "$pty_dir" 2>/dev/null || :
    PTY_PID=""
    PTY_INPUT=""
    return "$status"
}
