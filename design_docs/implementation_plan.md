# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-109 - split Rust toolchains and test the MSRV

State: NEW

Goal:

- Keep Rust 1.88 as the declared MSRV and release-build compiler, move local
  development and non-MSRV CI to Rust 1.97.1, and make the MSRV CI gate run the
  complete applicable Rust test suite instead of only a compile check.

Dependencies:

- No dependency on another planned task.
- Operator prerequisite before implementation: the invalid GitHub ref
  `refs/heads/dependabot/github_actions/dtolnay/rust-toolchain-1.100.0`
  currently points to commit `d41e0a51013a24261292e4065cab2f8fef784460` and must
  be removed from GitHub. Igor must tell Nev to perform that Git/GitHub
  operation; Igor must not perform it. Nev must delete that branch only if it
  still resolves to that exact commit. If the branch is already absent, Nev must
  verify that it is absent and proceed. If it resolves to any other commit, Nev
  must stop and ask for the task to be updated; no other ref may be deleted. The
  resulting source baseline is main-line commit
  `4266556884dad1cfea48862d272c317a51d23bc5`; implementation retains this
  planning commit as its separate documented ancestor.

Scope:

- Change exactly the `msrv` recipe in `justfile`. Keep its existing Rust 1.88
  installation guard and compile check, then run this exact command:
  `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +1.88 test --locked --all-targets --all-features`.
  This command covers the unit, integration, binary, example, and benchmark
  targets selected by Cargo's `--all-targets --all-features` selection. It does
  not run documentation tests; do not describe it as doing so and do not replace
  it with nextest or a narrower target selection.

- After that command, run this exact separate documentation-test command:
  `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +1.88 test --locked --all-features --doc`.
  Keep it separate because Cargo does not allow `--doc` with the other target
  selectors.

- Change `rust-toolchain.toml`'s channel to exactly `1.97.1`. This is the local
  development toolchain; it must not change `Cargo.toml`'s
  `rust-version = "1.88"`.

- In `.github/workflows/ci.yml`, set exactly these jobs to
  `dtolnay/rust-toolchain@1.97.1`: `check`, `acceptance`, `lint-all`, and
  `macos`. Keep `msrv` at `dtolnay/rust-toolchain@1.88.0` and keep `stable` on
  `dtolnay/rust-toolchain@stable`.

- In `.github/workflows/release.yml`, keep the release build job's toolchain,
  `rustup target`, and `cargo +... build` commands at Rust 1.88.0. Set the
  separate Linux release-quality-tools action to
  `dtolnay/rust-toolchain@1.97.1`. Do not change release target names or release
  behavior.

- In `.github/dependabot.yml`, leave the Cargo update entry unchanged and add
  exactly this block under the existing `package-ecosystem: github-actions`
  update entry:

  ```yaml
  ignore:
    - dependency-name: dtolnay/rust-toolchain
  ```

  Do not add an ignore entry to the Cargo update entry or add any other
  Dependabot ignore rule. This prevents Dependabot from proposing compiler
  toolchain ref updates; Igor must tell Nev to review such updates deliberately.

- Do not change `Cargo.toml`, `Cargo.lock`, `src/`, tests, or package version
  metadata. Do not add retries, failure suppression, conditional test skipping,
  or a separate test job that could leave the `msrv` recipe passing when either
  MSRV test command fails.

Acceptance criteria:

- `just msrv` installs or reuses Rust 1.88, runs the existing locked compile
  check, then runs the exact locked all-target/all-feature command and the exact
  locked documentation-test command in Scope. A failure from the compile check
  or either test command makes `just msrv` fail.
- The all-target command executes with `rustc --version` reporting 1.88.x, uses
  the committed lockfile, enables all Cargo features and targets, and covers the
  repository's unit, integration, binary, example, and benchmark targets. The
  separate `--doc` command executes with Rust 1.88.x and is the evidence for
  documentation-test coverage; neither command changes `Cargo.lock` or source
  files.
- `rust-toolchain.toml` reports Rust 1.97.1 for local development, and the
  `check`, `acceptance`, `lint-all`, and `macos` CI jobs use Rust 1.97.1. The
  `msrv` job still uses Rust 1.88.0, and the release build still uses Rust
  1.88.0.
- The `.github/workflows/ci.yml` `msrv` job continues to select
  `dtolnay/rust-toolchain@1.88.0` and invokes `just --no-deps msrv`; the job
  therefore proves the compile, all-target, and documentation MSRV gates rather
  than running only the newer normal-toolchain jobs.
- The `.github/dependabot.yml` GitHub Actions update entry contains exactly the
  ignore block specified in Scope, the Cargo entry has no ignore rule, and no
  other ignore rule is added.
- A locally run `just msrv` passes, Rust 1.97.1 can compile the project with
  `cargo +1.97.1 check --locked`, and the relevant quality gates for the
  `justfile`, TOML, YAML, and Dependabot changes pass. No package version bump
  is made because this task changes only toolchain and test-verification
  configuration and does not modify non-test application source under `src/`.

## TASK-108 - add NixOS and Home Manager installation

State: NEW

Goal:

- Make released Stay binaries installable on NixOS and through Home Manager on
  Linux and macOS, without requiring a source build, Rust, Cargo, or a compiler.
  Provide a reproducible Nix flake that packages the matching target-native
  GitHub Release archive, exposes the package for supported host systems, and
  supplies module examples that install Stay and its required tmux runtime
  dependency declaratively.

Dependencies:

- TASK-107 must remain `COMPLETED`; its release workflow and the `v0.0.86`
  GitHub Release assets are the source of the initial Nix package URLs and
  checksums.
- The Nix implementation must use the already published `v0.0.86` assets before
  any future release automation is designed. A later release update must update
  the package version, target URLs, and hashes together.
- This task does not modify non-test application source under `src/`, so its
  implementation commit must not bump the package version or alter Cargo
  metadata. Every Nix package and version assertion remains fixed at `v0.0.86`
  and `stay 0.0.86`.

Scope:

- In the application repository, add a root `flake.nix` and `flake.lock`. The
  flake must use the stable NixOS 26.05 line by pinning `nixpkgs` to commit
  `9f78f44a87948854445dae0b6bf82b2e87e4efb5` from `github:NixOS/nixpkgs`, and
  use the stable Home Manager 26.05 line by pinning `home-manager` to commit
  `d4fd24667c8cbef124bb70a20380cab75ec8474d` from
  `github:nix-community/home-manager`. The flake must set
  `inputs.home-manager.inputs.nixpkgs.follows = "nixpkgs"`, and the lock file
  must contain those exact revisions with no floating branch or tag. It must
  expose a `packages` attribute for exactly these host systems: `x86_64-linux`,
  `aarch64-linux`, `x86_64-darwin`, and `aarch64-darwin`. It must also expose a
  default package for each of those systems, `nixosModules.stay`,
  `homeManagerModules.stay`, and these check attributes for every system:
  `stay-package`, `release-hashes`, `nixos-flake`, `nixos-legacy`,
  `home-manager-flake`, `home-manager-legacy`, and `home-manager-embedded`. The
  flake module outputs must be functions accepting `{ pkgs, stay }`, and their
  `programs.stay.package` option must default to `stay` while allowing a module
  consumer to assign another package explicitly.

- Add exactly these legacy Nix files: `nix/default.nix`, `nix/package.nix`,
  `nix/nixos-module.nix`, and `nix/home-manager-module.nix`. `nix/default.nix`
  must accept a `pkgs` argument, expose the `stay` package and both modules as
  `stay`, `nixosModule`, and `homeManagerModule` attributes, and support both
  caller-provided nixpkgs forms: `-I nixpkgs=...` and `--arg pkgs ...`. The
  legacy module functions must accept `{ pkgs, stay }` and use the same package
  override behavior as the flake module outputs. Neither form may silently fetch
  an unpinned source tree or build Stay from source.

- Implement the package only in `nix/package.nix`. It must select exactly one
  matching Stay release archive from an explicit `system` argument. The flake
  must pass its package system, and `nix/default.nix` must pass
  `builtins.currentSystem`. Linux x86_64 must use `x86_64-unknown-linux-gnu`,
  Linux ARM64 must use `aarch64-unknown-linux-gnu`, macOS x86_64 must use
  `x86_64-apple-darwin`, and macOS ARM64 must use `aarch64-apple-darwin`. Each
  URL must point to the `v0.0.86` GitHub Release and use the exact corresponding
  SHA-256 hash from `SHA256SUMS`, expressed in the hash format required by the
  pinned nixpkgs.

  The initial package data is fixed and must appear exactly as follows. The
  package must use the listed SRI value in its `fetchurl` call. CI must compare
  each fetched archive with the listed hexadecimal value and this exact manifest
  URL: `https://github.com/nevdelap/stay/releases/download/v0.0.86/SHA256SUMS`:

  | system           | release asset URL                                                                                          | SHA-256                                                            | Nix SRI                                               |
  | ---------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------- |
  | `x86_64-linux`   | `https://github.com/nevdelap/stay/releases/download/v0.0.86/stay-v0.0.86-x86_64-unknown-linux-gnu.tar.gz`  | `64c63305fefacb696647880b7cf30baf94e9c10365630878f03d4adf2e4c02ce` | `sha256-ZMYzBf76y2lmR4gLfPMLr5TpwQNlYwh48D1K3y5MAs4=` |
  | `aarch64-linux`  | `https://github.com/nevdelap/stay/releases/download/v0.0.86/stay-v0.0.86-aarch64-unknown-linux-gnu.tar.gz` | `9daf0b200f696c646b20b2a1c1b5905bbda6771ccb663ebb058329b4d0f640da` | `sha256-na8LIA9pbGRrILKhwbWQW72mdxzLZj67BYMptND2QNo=` |
  | `x86_64-darwin`  | `https://github.com/nevdelap/stay/releases/download/v0.0.86/stay-v0.0.86-x86_64-apple-darwin.tar.gz`       | `bd9793b85d13da05472e634418d3722f7d8b3b038a3b125a0abe8d36d86fbe2a` | `sha256-vZeTuF0T2gVHLmNEGNNyL32LOwOKOxJaCr6NNthvvio=` |
  | `aarch64-darwin` | `https://github.com/nevdelap/stay/releases/download/v0.0.86/stay-v0.0.86-aarch64-apple-darwin.tar.gz`      | `5e313ab7dbbe53329635587551a22f39f5b61f18de0d72d886363b88facedee0` | `sha256-XjE6t9u+UzKWNVh1UaIvOfW2HxjeDXLYhjY7iPrO3uA=` |

- Package the archive as a prebuilt executable with `stdenvNoCC.mkDerivation`,
  `fetchurl`, and the exact SRI hash from the table. Install only the top-level
  `stay` executable into `$out/bin/stay`; do not compile source, invoke Cargo or
  Rust, include a source-build fallback, or hard-code a user or system prefix.
  On Linux, add the pinned nixpkgs `autoPatchelfHook` and every dynamic library
  required by the selected release ELF to `nativeBuildInputs` and `buildInputs`,
  so the installed executable has a valid Nix store ELF interpreter. On macOS,
  preserve the native Mach-O executable and do not apply Linux ELF patching.
  Assert that the selected nixpkgs `tmux` version is at least 3.6. The package
  must set `propagatedBuildInputs = [ pkgs.tmux ]` so tmux is in the runtime
  closure. Both modules must use exactly `pkgs.tmux` and add it to the resulting
  system/user package list by default so `tmux` is on `PATH`. There is no module
  option for selecting another tmux package.

- Implement the NixOS module only in `nix/nixos-module.nix`. It must be
  importable from a traditional non-flake `configuration.nix` and from the
  flake's NixOS module output. Define exact options `programs.stay.enable`
  (default `true`), `programs.stay.package` (defaulting to the package), and
  `programs.stay.enableTmux` (default `true`). When enabled, install the Stay
  package in `environment.systemPackages`; when `programs.stay.enableTmux` is
  true, also install `pkgs.tmux`. With `programs.stay.enable = false`, install
  neither package regardless of `enableTmux`. With `enable = true` and
  `enableTmux = false`, install only Stay. The module must not enable an
  unrelated service, change the user's shell, or silently create a daemon; this
  task is for installation, not service management.

- Implement the Home Manager module only in `nix/home-manager-module.nix`. It
  must be importable from a traditional non-flake `home.nix`, standalone flake
  Home Manager, and a NixOS configuration using `home-manager.nixosModules`.
  Define exact options `programs.stay.enable` (default `true`),
  `programs.stay.package` (defaulting to the package), and
  `programs.stay.enableTmux` (default `true`). When enabled, install the Stay
  package in `home.packages`; when `programs.stay.enableTmux` is true, also
  install `pkgs.tmux`. With `programs.stay.enable = false`, install neither
  package regardless of `enableTmux`. With `enable = true` and
  `enableTmux = false`, install only Stay. The module must work on non-NixOS
  Linux, NixOS, and macOS. It must not require Home Manager to manage shell
  dotfiles or enable an unrelated service.

- Implement the named flake checks exactly as follows. `stay-package` builds
  `packages.<system>.stay` and `packages.<system>.default`, asserts they are
  identical outputs containing only `bin/stay`, asserts the propagated tmux
  closure and the absence of Cargo, Rust, compilers, and source trees, and runs
  `stay --version` as `stay 0.0.86`. `release-hashes` fetches the exact
  `SHA256SUMS` URL, requires exactly four lines, and requires each listed
  filename to have its literal task-scope hash. `nixos-flake` and `nixos-legacy`
  evaluate the NixOS module with defaults, `enable = false`, and
  `enableTmux = false`. `home-manager-flake` and `home-manager-legacy` do the
  same for standalone Home Manager on Linux and macOS. `home-manager-embedded`
  evaluates Home Manager embedded in NixOS. The package and hash checks run on
  the native runner for their `<system>`; all module checks run as evaluation
  checks on each matrix runner, and no cross-system binary execution or
  emulation is permitted.

- Add a Nix-focused section to exactly `README.md`, documenting with complete
  commands both installation styles. For flake users document `nix run` and
  `nix profile install`, a flake-based NixOS `environment.systemPackages`
  configuration, standalone flake-based Home Manager using `home.packages`, and
  Home Manager imported into NixOS. For non-flake users document the legacy
  package entrypoint with `nix-build` and `nix-env`, a traditional NixOS
  `configuration.nix` import, and a traditional standalone `home.nix` import.
  State explicitly that the package downloads the target-native GitHub Release
  binary, that tmux is a runtime dependency, that Stay requires tmux 3.6 or
  newer, and that the release-pinned hashes provide integrity checking.

- Add CI coverage in exactly `.github/workflows/ci.yml`; do not add a new
  workflow file. The CI must evaluate the flake and build each package from the
  pinned inputs without compiling Stay from source. It must exercise the NixOS
  module and the Home Manager module through both flake and legacy evaluation
  checks, and must run the installed binary's version check on each native
  runner assigned to its package system. Use the native runner mapping
  `x86_64-linux` → `ubuntu-24.04`, `aarch64-linux` → `ubuntu-24.04-arm`,
  `x86_64-darwin` → `macos-15-intel`, and `aarch64-darwin` → `macos-14`; assert
  each runner architecture before building or executing its package. CI must
  fail on a wrong archive URL, hash, system mapping, missing `bin/stay`, missing
  tmux runtime dependency, or accidental Rust/Cargo build path.

- Keep the existing Cargo, Homebrew, release, runtime, and shell-integration
  documentation and behavior intact. Do not change application source behavior,
  release asset names, existing tags, or the Homebrew tap as part of this task.
  Every GitHub operation, including any CI or release verification, follows the
  repository rule that Igor tells Nev to perform it.

Acceptance criteria:

- On `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, and `aarch64-darwin`, the
  pinned flake uses exactly the Nixpkgs and Home Manager revisions specified in
  Scope, and `inputs.home-manager.inputs.nixpkgs` resolves to the same locked
  Nixpkgs node. It evaluates and exposes `packages.<system>.default` and
  `packages.<system>.stay`, and each output is a runnable package containing
  exactly the Stay executable at `bin/stay`.
- The four package definitions select exactly the four matching `v0.0.86`
  release archives and the four literal SHA-256 values in the task scope: no
  system selects another system's archive, no URL is unversioned, and no hash is
  omitted or replaced by an insecure placeholder. The values must also match the
  published `SHA256SUMS` file.
- Building each package with the pinned flake downloads the release binary and
  does not invoke Cargo, Rust, a compiler, a source checkout, or a source-build
  fallback. The resulting package provides tmux in its runtime dependency
  closure, and `stay --version` reports exactly `stay 0.0.86` on all four
  package-native runners.
- The legacy package entrypoint evaluates with an explicit caller-provided
  nixpkgs and supports `nix-build` and `nix-env` installation without flakes. It
  exposes exactly `stay`, `nixosModule`, and `homeManagerModule`; the module
  imports accept `{ pkgs, stay }`, default `programs.stay.package` to `stay`,
  and permit an explicit `programs.stay.package` override. It asserts tmux 3.6
  or newer, and neither path invokes Cargo, Rust, a compiler, or a source
  checkout.
- A traditional non-flake NixOS `configuration.nix` can import the NixOS module
  and, with its defaults, installs both Stay and tmux through
  `environment.systemPackages`; module evaluation succeeds without enabling a
  service, changing shell configuration, or requiring Home Manager. Setting
  `programs.stay.enable = false` removes Stay, and setting
  `programs.stay.enableTmux = false` does not add tmux.
- A standalone traditional non-flake `home.nix` can import the Home Manager
  module and, with its defaults, installs both Stay and tmux through
  `home.packages` on Linux and macOS, including non-NixOS Linux; the module
  evaluates without requiring NixOS-only options. The same enable options have
  the documented disabling behavior: `programs.stay.enable = false` removes
  Stay, and `programs.stay.enableTmux = false` does not add tmux.
- Flake-based NixOS, standalone Home Manager, and Home Manager embedded in NixOS
  can import `nixosModules.stay` and `homeManagerModules.stay` and install the
  same package, with explicit `programs.stay.package` overrides working in each
  module context.
- The documented commands are complete and executable for both
  `nix run`/`nix profile install` and legacy `nix-build`/`nix-env`, plus both
  flake and non-flake NixOS and Home Manager configurations. They identify the
  supported systems and state the target-native binary, tmux 3.6 minimum, and
  hash integrity behavior.
- Nix CI runs the `stay-package`, `release-hashes`, `nixos-flake`,
  `nixos-legacy`, `home-manager-flake`, `home-manager-legacy`, and
  `home-manager-embedded` checks for each of the four matrix systems in
  `.github/workflows/ci.yml`. The package and hash checks run natively for the
  matrix system, module checks evaluate all specified contexts, and the checks
  fail for an incorrect platform mapping, archive hash, package output, module
  option, runtime dependency, or source-build path.
- Existing Cargo and Homebrew installation instructions remain present and
  accurate, no application source behavior changes are included, and the
  applicable documentation/workflow quality gates pass. Because this task
  changes Nix files, CI configuration, and installation documentation but no
  Rust source, `just qlint` is required; `just qcheck` and `just mac-qcheck` are
  not required unless the final diff also changes Rust source, tests, or
  manifests.
