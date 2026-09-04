# Development loop

Prerequisites for the local loop are Rust, `just`, `uv`, Docker, tmux, `curl`,
`tar`, and `ripgrep` (`rg`). Run `just install` to install the hook, download
the prebuilt `cargo-nextest` binary, and fetch locked Rust dependencies. The
install recipe does not install the system tools or optional `cargo-sweep`.

For changes to `flake.nix`, `nix/`, or Nix installation documentation, run the
optional Linux diagnostic:

```sh
just nix-diagnostic
```

It runs the package builds and `nix flake check --system x86_64-linux` inside
the pinned `nixos/nix:2.35.2` Docker image. The Nix store is kept in the
`stay-nix-2.35.2` Docker volume so later runs do not redownload dependencies.
Remove that volume with `docker volume rm stay-nix-2.35.2` when reclaiming the
cache. The diagnostic is intended to catch Linux evaluation and packaging
failures before pushing; it does not replace native CI, which remains the
authoritative check for all four Linux and Darwin architectures.

Use `just context` first. It lists the current worktree files, quality groups,
likely tests, the fast gate, and the full handoff gate.

## Quality checks

Changed-file quality prefers staged paths. With no staged changes, it falls back
to the current commit's parent diff; it does not select ordinary unstaged edits.
Stage the files you want checked before running the recipes:

```sh
git add path/to/changed-file
just qlint
```

`qlint` already runs formatting before linting. Use `qformat` by itself when you
only need formatting; do not normally run `qformat` followed by `qlint`, because
that formats twice. If formatting changes a file, inspect the diff, stage the
accepted result, and rerun the recipe until it is clean.

Rust commands use the tracked `rust-toolchain.toml` toolchain. The debugging
check rejects `dbg!`, `todo!`, `unimplemented!`, and print macros in `src/` and
`tests/`. Intentional user-visible or test-protocol output must put the exact
`// quality: intentional-output` marker immediately above its macro.

For a formatting-only edit:

```sh
just qformat
```

For the normal narrow quality-and-test loop:

```sh
just qcheck-fast
```

This runs changed-file quality and nextest, but deliberately omits MSRV. The
pre-push hook uses this fast gate.

## Test recipes

Run one integration target with nextest when possible:

```sh
just test-target attachment
just test-filter picker
```

`test` uses Cargo's standard test runner. `test-nextest` runs the same broad
test selection with nextest. `test-target` selects one integration-test target;
`test-filter` selects tests using a nextest expression.

## Full checks and maintenance

`check-nextest` runs changed-file quality, nextest, and MSRV. Its quiet wrapper
is `qcheck-nextest`. The full Rust handoff is:

```sh
just qcheck
just mac-qcheck
```

`qcheck` uses the standard Cargo test runner and includes MSRV. `mac-qcheck`
runs the configured macOS Rust test gate.

Acceptance-layer changes use the corresponding Bats handoff:

```sh
just qacceptance
just mac-qacceptance
```

These recipes build the release binary and run the repository's acceptance
wrapper with isolated tmux state and diagnostics. A mixed Rust and acceptance
change requires all four gates. Documentation-only changes require only their
documentation formatting and lint checks. These decisions are made from the
final diff; any gate-relevant change after a passing run invalidates the earlier
result.

CI runs changed-file quality, nextest, MSRV, and the acceptance matrix through
the same wrapper; local handoff recipes are still required for the final task
snapshot.

Set each CI job's `timeout-minutes` from the latest successful run: measure the
job's wall-clock runtime, multiply it by two, and round up to the next whole
minute. Matrix jobs should use a timeout value for each platform when their
runtimes differ. Recheck these values after meaningful changes to the job or its
dependencies.

The `format-all`, `lint-all`, and `check-all` recipes deliberately operate on
the whole repository and are mainly for maintenance or final verification.
`msrv` checks Rust 1.89. `update-rust`, `update-lock`, and `sweep` are explicit
maintenance operations; review their effects before using them. `sweep` uses
`cargo-sweep` when available and otherwise falls back to `cargo clean`.

`build` and `build-release` compile debug and optimized binaries respectively;
`run` executes the debug binary with the supplied arguments.
