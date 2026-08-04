# Development loop

Prerequisites for the local loop are Rust, `just`, `uv`, Docker, tmux, `curl`,
`tar`, and `ripgrep` (`rg`). Run `just install` to install the hook, download
the prebuilt `cargo-nextest` binary, and fetch locked Rust dependencies. The
install recipe does not install the system tools or optional `cargo-sweep`.

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
is `qcheck-nextest`. The full handoff remains:

```sh
just qcheck
just mac-qcheck
```

`qcheck` uses the standard Cargo test runner and includes MSRV. `mac-qcheck`
runs the configured macOS test gate. CI currently runs changed-file quality,
nextest, and MSRV; it does not run `qcheck` or `mac-qcheck` directly.

The `format-all`, `lint-all`, and `check-all` recipes deliberately operate on
the whole repository and are mainly for maintenance or final verification.
`msrv` checks Rust 1.88. `update-rust`, `update-lock`, and `sweep` are explicit
maintenance operations; review their effects before using them. `sweep` uses
`cargo-sweep` when available and otherwise falls back to `cargo clean`.

`build` and `build-release` compile debug and optimized binaries respectively;
`run` executes the debug binary with the supplied arguments.
