# Bounded tmux diagnostics for the Bats acceptance suite.

_acceptance_tmux_validate_socket_root() {
    : "${TMUX_TMPDIR:?ci-run-acceptance.sh must provide TMUX_TMPDIR}"
    [[ -d "$TMUX_TMPDIR" ]] || {
        echo "TMUX_TMPDIR must be an existing directory" >&2
        return 1
    }

    local tmp_root resolved_tmpdir
    tmp_root="${TMPDIR:-/tmp}"
    tmp_root="$(cd "$tmp_root" && pwd -P)"
    resolved_tmpdir="$(cd "$TMUX_TMPDIR" && pwd -P)"
    case "$resolved_tmpdir" in
        "$tmp_root"/stay-acceptance.*) ;;
        *)
            echo "TMUX_TMPDIR is not a stay acceptance directory" >&2
            return 1
            ;;
    esac
}

acceptance_tmux_wait_until_output() {
    if (($# != 2)); then
        echo "usage: acceptance_tmux_wait_until_output SESSION MARKER" >&2
        return 2
    fi
    _acceptance_tmux_validate_socket_root || return

    local session="$1" marker="$2" output attempt
    for attempt in {1..100}; do
        : "$attempt"
        if output="$(tmux -L stay -f /dev/null capture-pane -p -t "$session" -S - -E - 2>/dev/null)" &&
            [[ "$output" == *"$marker"* ]]; then
            return 0
        fi
        sleep 0.1
    done
    echo "timed out waiting for tmux output: $marker" >&2
    tmux -L stay -f /dev/null capture-pane -p -t "$session" -S - -E - >&2 2>/dev/null || :
    return 1
}

acceptance_tmux_wait_until_client_flag() {
    if (($# != 2)); then
        echo "usage: acceptance_tmux_wait_until_client_flag SESSION FLAG" >&2
        return 2
    fi
    _acceptance_tmux_validate_socket_root || return

    local session="$1" flag="$2" output client_session client_flags attempt
    for attempt in {1..100}; do
        : "$attempt"
        if output="$(tmux -L stay -f /dev/null list-clients \
            -F '#{client_session}:#{client_flags}' 2>/dev/null)"; then
            while IFS=: read -r client_session client_flags; do
                if [[ "$client_session" == "$session" ]] &&
                    [[ ",$client_flags," == *",$flag,"* ]]; then
                    return 0
                fi
            done <<<"$output"
        fi
        sleep 0.1
    done
    echo "timed out waiting for tmux client $session to have flag $flag" >&2
    tmux -L stay -f /dev/null list-clients \
        -F '#{client_session}:#{client_flags}' >&2 2>/dev/null || :
    return 1
}
