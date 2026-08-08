# stay

stay keeps terminal work in named tmux sessions that survive a dropped
connection, a closed terminal, or a later re-attach. It provides an interactive
picker for everyday use and explicit commands for scripts, while preserving the
session's scrollback and allowing output to be logged when needed.

## Installation

stay requires tmux 3.6 or newer. Install tmux with your operating system's
package manager. For a published release, install stay from crates.io:

```sh
cargo install stay
```

To install the version from a checkout instead:

```sh
git clone https://github.com/nevdelap/stay.git
cd stay
cargo install --path .
```

## Commands

Running `stay` without a subcommand opens the interactive session picker:

```sh
stay
```

The explicit commands are useful in scripts or when a session name is already
known:

```sh
# List sessions for a human, or for a script.
stay list
stay list --json

# Create a session, optionally specifying its initial command.
stay create build
stay create tests cargo test

# Attach to an existing session.
stay attach build

# Kill an existing session.
stay kill build
```

The JSON `created_at` and `terminated_at` fields use RFC 3339 timestamps in UTC,
with a trailing `Z`. Human-readable terminated-session rows use the same UTC
representation.

`stay create` also accepts `--cwd DIR`, `--force-recreate`, and `--attach`. When
attaching, `stay attach` supports `--read-only`, `--low-priority`, `--log FILE`,
`--truncate`, and `--raw`.

### Picker keys

The picker supports these key bindings. The compact status panel intentionally
omits `c` and `q` in favor of the primary create-row and `Esc` affordances:

| Key        | Action                                                                      |
| ---------- | --------------------------------------------------------------------------- |
| Up/Down    | Select a session                                                            |
| `v`        | Toggle view-only attach                                                     |
| `l`        | Toggle low-priority attach                                                  |
| `/`        | Enter fuzzy filter mode                                                     |
| `c`        | Create a session                                                            |
| Enter      | Attach to the selected session                                              |
| `r`        | Recreate the selected terminated session, or recreate it directly when live |
| `e`        | Edit the selected session name                                              |
| `k`        | Kill the selected session                                                   |
| `K`        | Kill all terminated sessions                                                |
| `q` or Esc | Quit                                                                        |

In filter mode, type a case-insensitive fuzzy query to narrow the session rows.
Enter attaches the selected match and Esc cancels filtering; the filter input
and `No matching sessions` state are not actionable rows.

### Configuration

The configuration file is `stay/config.toml` below the platform's user config
directory: typically `~/.config/stay/config.toml` on Linux and
`~/Library/Application Support/stay/config.toml` on macOS. The supported TOML
keys are:

| Key                            | Description                                        |
| ------------------------------ | -------------------------------------------------- |
| `default_command`              | Command used when a session is created without one |
| `detach_key`                   | Control key that detaches from a session           |
| `copy_mode_key`                | Control key that enters tmux copy mode             |
| `history_lines`                | Number of lines to retain, or `"unlimited"`        |
| `log_capture_interval_seconds` | Interval between log captures                      |

Environment variables override the corresponding file settings: `STAY_CMD`,
`STAY_DETACH_KEY`, `STAY_COPY_MODE_KEY`, `STAY_HISTORY_LINES`, and
`STAY_LOG_CAPTURE_INTERVAL_SECONDS`.

By default, `Ctrl+\` detaches and `Ctrl+Space` enters copy mode. Change either
with `detach_key` or `copy_mode_key` in the config file, or with
`STAY_DETACH_KEY` or `STAY_COPY_MODE_KEY`. Control keys use names such as
`Ctrl+X`, `Ctrl+Space`, and `Ctrl+[`. The two configured keys must be distinct.

## Troubleshooting

### Recovering a deleted tmux socket

If you manually delete tmux's own server socket while a stay session is running,
the session is not lost. Send `SIGUSR1` to the running tmux server and tmux will
recreate the socket in place:

```sh
tmux_pid="$(tmux -L stay display-message -p '#{pid}')"
kill -USR1 "$tmux_pid"
```

Run the first command while the socket is still present. If it has already been
deleted, find the `tmux -L stay` server with `ps` or `pgrep` and send `SIGUSR1`
to that process instead. The server itself remains running when its socket is
deleted, so its sessions and data are preserved.
