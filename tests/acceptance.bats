#!/usr/bin/env bats

bats_require_minimum_version 1.14.0

# Use Stay for all session operations; DO NOT call tmux directly in this file.
stay() {
    cargo run --release --locked --quiet -- "$@"
}

setup_file() {
    tmux_tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/stay-acceptance.XXXXXX")
    export TMUX_TMPDIR="$tmux_tmpdir"
    # Keep Stay and tmux from treating the acceptance shell as an attached client.
    unset TMUX
}

cleanup_stay_sessions() {
    local session
    {
        stay list 2>/dev/null |
            sed -E 's/[[:space:]]+\[(detached|attached|terminated.*)\]$//' || :
        # Keep cleanup targeted if Stay cannot inventory the server.
        printf '%s\n' human-readable-one human-readable-two
    } | while IFS= read -r session; do
        [[ -n "$session" ]] || continue
        stay kill "$session" >/dev/null 2>&1 || :
    done
}

teardown() {
    cleanup_stay_sessions
}

teardown_file() {
    rm -rf "$tmux_tmpdir"
}

@test "stay list shows named sessions as detached human-readable rows" {
    run stay create human-readable-one sleep 60
    [ "$status" -eq 0 ]

    run stay create human-readable-two sleep 60
    [ "$status" -eq 0 ]

    run stay list
    [ "$status" -eq 0 ]
    [ "$output" = $'human-readable-one [detached]\nhuman-readable-two [detached]' ]
}
