# Review: TASK-112

## Findings

### R001

Status: ADDRESSED

The documented `nix-build` command in `README.md:164-165` passes
`--arg pkgs 'import <nixpkgs> {}'`, but its `-E` expression independently
constructs the package with `pkgs = import <nixpkgs> {}`. The command-line
argument is therefore unused and the example still requires a configured
`<nixpkgs>` path; it does not demonstrate the explicit `pkgs` invocation form
required by the legacy installation scope. A user without `<nixpkgs>` cannot
make this example work by supplying the advertised `--arg`. Use an expression
or file/attribute selection that consumes the `pkgs` argument, and show a
caller-provided nixpkgs path that is not still hidden behind `<nixpkgs>`.

On the follow-up snapshot, the expression was changed to
`(import ./nix/default.nix { inherit pkgs; }).stay`, but it still has no
function parameter that `nix-build --arg pkgs ...` can apply. The free `pkgs`
identifier remains unbound in this `-E` expression, so the documented command
still does not execute. Use a function expression such as
`pkgs: (import ./nix/default.nix { inherit pkgs; }).stay`, or invoke the
`default.nix` file with `-A stay` so the command-line argument is applied to
the file's function.

The current snapshot changes the expression to
`pkgs: (import ./nix/default.nix { inherit pkgs; }).stay` and supplies
`import /path/to/nixpkgs {}` as the `--arg pkgs` value. This binds and consumes
the explicit package-set argument, so the documented `nix-build` command no
longer depends on `<nixpkgs>`. The finding is addressed.

## Verification

- `just qlint`: passed and left the worktree clean.
- Follow-up review: the `pkgs:` lambda now consumes `--arg pkgs`.
- No source, test, fixture, or manifest files changed in the task commit.
- The inherited CI workflow contains the required four native Nix matrix
  systems and the exact `nix build` and `nix flake check --system "$SYSTEM"`
  commands; the local environment has no `nix` executable, so those native
  commands were not rerun locally.
- Renewal review: `just nix-diagnostic` passed the pinned x86_64-linux Docker
  package build and all flake checks. GitHub CI also passed the authoritative
  native Linux and Darwin matrix.
- The built package payload contains exactly `bin/stay` and the uncompressed
  `share/man/man1/stay.1`; the additional `nix-support` file is Nix closure
  metadata, so the flake check correctly scopes its payload count to `bin` and
  `share`.

## Final decision

Status: COMPLETED
