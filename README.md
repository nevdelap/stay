# stay

stay keeps terminal work in named tmux sessions that survive a dropped
connection, a closed terminal, or a later re-attach. It provides an interactive
picker for everyday use and explicit commands for scripts, while preserving the
session's scrollback and allowing output to be logged when needed.

## Installation

stay requires tmux 3.6 or newer.

### Brew on Linux and Mac

For a published binary release on macOS or Linux, install stay from the Homebrew
tap:

```sh
brew tap nevdelap/stay
brew install nevdelap/stay/stay
```

The tap downloads a target-native Stay binary archive from the Stay GitHub
Release; it does not build Stay from source. Homebrew supplies tmux as a
dependency, but Stay still requires tmux 3.6 or newer.

The Homebrew install also provides the `stay(1)` manual page; read it with
`man stay`.

### Cargo

Install stay from crates.io:

```sh
cargo install stay
```

### NixOS and Home Manager

The Nix package downloads the target-native Stay binary from the GitHub Release;
it does not build Stay from source. It includes tmux as a runtime dependency,
and Stay requires tmux 3.6 or newer. The release-pinned hashes provide integrity
checking for each archive.

With flakes enabled, run Stay directly or install it into a profile:

```sh
nix run github:nevdelap/stay
nix profile install github:nevdelap/stay
```

A flake-based NixOS configuration can install Stay and tmux with the module:

```nix
{
  inputs.stay.url = "github:nevdelap/stay";

  outputs = { self, nixpkgs, stay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      stayPackage = stay.packages.${system}.stay;
    in {
      nixosConfigurations.example = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          (stay.nixosModules.stay {
            inherit pkgs;
            stay = stayPackage;
          })
        ];
      };
    };
}
```

For standalone flake-based Home Manager, import the Home Manager module:

```nix
{ pkgs, inputs, ... }:
{
  home.username = "alice";
  home.homeDirectory = "/home/alice";
  home.stateVersion = "26.05";

  imports = [
    (inputs.stay.homeManagerModules.stay {
      inherit pkgs;
      stay = inputs.stay.packages.${pkgs.system}.stay;
    })
  ];
}
```

When Home Manager is embedded in NixOS, add the same module to the user's
imports:

```nix
{ pkgs, inputs, ... }:
{
  imports = [ inputs.home-manager.nixosModules.home-manager ];
  home-manager.users.alice = {
    imports = [
      (inputs.stay.homeManagerModules.stay {
        inherit pkgs;
        stay = inputs.stay.packages.${pkgs.system}.stay;
      })
    ];
  };
}
```

In the flake that evaluates a standalone Home Manager configuration, pass the
flake inputs as module arguments:

```nix
homeConfigurations.alice = home-manager.lib.homeManagerConfiguration {
  pkgs = import nixpkgs { system = "x86_64-linux"; };
  extraSpecialArgs = { inherit inputs; };
  modules = [ ./home.nix ];
};
```

For Home Manager embedded in NixOS, pass the inputs as NixOS module arguments:

```nix
nixosConfigurations.example = nixpkgs.lib.nixosSystem {
  specialArgs = { inherit inputs; };
  modules = [ ./configuration.nix ];
};
```

Without flakes, the legacy entrypoint accepts either an explicit nixpkgs
argument or the caller's `<nixpkgs>` path:

```sh
nix-build --arg pkgs 'import <nixpkgs> {}' \
    -E '(import ./nix/default.nix { pkgs = import <nixpkgs> {}; }).stay'
nix-env -f ./nix/default.nix -iA stay --arg pkgs 'import <nixpkgs> {}'
```

A traditional NixOS `configuration.nix` imports `nix/nixos-module.nix` with the
package from `nix/default.nix`:

```nix
{ pkgs, ... }:
let
  stay = import /path/to/stay/nix/default.nix { inherit pkgs; };
in
{
  imports = [
    (import /path/to/stay/nix/nixos-module.nix {
      inherit pkgs;
      inherit (stay) stay;
    })
  ];
}
```

A traditional standalone `home.nix` uses the corresponding Home Manager module
and works on Linux, including non-NixOS Linux, and macOS:

```nix
{ pkgs, ... }:
let
  stay = import /path/to/stay/nix/default.nix { inherit pkgs; };
in
{
  imports = [
    (import /path/to/stay/nix/home-manager-module.nix {
      inherit pkgs;
      inherit (stay) stay;
    })
  ];
}
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

`stay create` also accepts `--cwd DIR`, `--force-recreate`, and `--attach`. With
`--attach`, `-r/--read-only` and `-L/--low-priority` select the corresponding
attach mode.

When attaching, `stay attach` supports `-r/--read-only`, `-L/--low-priority`,
`-p/--pass-through`, `--log FILE`, `--truncate`, and `--raw`. Pass-through
forwards stdin incrementally to the session without attaching; it cannot be
combined with the other attach modifiers.

### Shell integration

`stay shell-integration` prints a POSIX shell snippet for the prompt helper.
Source it from a shell startup file, for example:

```sh
eval "$(stay shell-integration)"
```

Use `stay shell-integration --s-alias` to also request `alias s=stay`; the alias
is omitted with a warning if it would conflict with an existing alias,
executable, or unreadable shell startup file. To enable the prompt segment, use
`eval "$(stay --prompt-integration)"` and reference `stay_prompt_segment` from
the shell prompt. In zsh, also enable `setopt PROMPT_SUBST`.

### Picker keys

The picker supports these key bindings:

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
