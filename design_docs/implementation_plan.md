# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-111 - install a man page with Homebrew

State: NEW

Goal:

- Make `brew install nevdelap/stay/stay` install a complete `stay(1)` manual
  page alongside the Stay executable on macOS and Linux, using the same
  target-native release archive as the binary and without requiring a source
  checkout or compiler.

Dependencies:

- No dependency on another planned task. This task is independent of the NixOS
  and Home Manager installation work in TASK-108.
- The application repository is currently at package version `0.0.87`; the
  updated release for this task is exactly `v0.0.88`, and the tag must match the
  package metadata after the application changes are merged and CI passes.
- Nev, acting as the human release owner, must perform every Git and GitHub
  operation for both repositories, including the application commit merge, the
  `v0.0.88` tag and release publication, the tap branch and commit, and the tap
  pull request. Igor must provide the exact handoff data and must not perform
  those external operations.
- The separate public tap repository `nevdelap/homebrew-stay` and its existing
  four-platform formula CI must be available. The tap formula commit must be
  made only after the `v0.0.88` release assets and `SHA256SUMS` exist.
- This is one coordinated TASK-111 with one canonical implementation commit in
  this application repository and one separately reviewed `TASK-111:` commit in
  `nevdelap/homebrew-stay`. The canonical application commit contains only the
  application man page, README, release workflow, package metadata, and man-page
  quality tooling; the tap commit contains only the formula and its formula
  test. The tap commit is not squashed into or copied into the application
  commit.
- The state and handoff sequence is explicit: after the application changes and
  their local gates pass, Igor sets this task to `IMPLEMENTED` and records the
  application commit SHA in the handoff. Nev then merges that commit, creates
  and publishes tag `v0.0.88`, and returns the release URL, all four archive
  URLs, the exact `SHA256SUMS` values, and archive-content/mode checks. Only
  after those assets exist does Nev create the tap commit and pull request, then
  return its commit SHA, pull-request URL, and the four-platform formula CI and
  `brew test` results. Rufus reviews the canonical application diff, the tap
  pull-request diff, and this complete evidence set before marking the task
  `COMPLETED`; until then an externally published or tap deliverable is not
  considered complete merely because the application commit is implemented. If
  Nev cannot perform a release step, the task remains `IMPLEMENTED` pending the
  human operator's decision to defer it.

Scope:

- In the application repository, add exactly `docs/stay.1` as a hand-written
  POSIX man page for `stay(1)`. It must document the command name, synopsis,
  interactive picker behavior, every public subcommand (`list`, `create`,
  `attach`, `kill`, and `shell-integration`), all public options and their
  conflicts, tmux 3.6 minimum requirement, configuration file and supported
  `STAY_*` environment variables, logging modes, pass-through behavior, shell
  integration, picker keys, exit/error behavior, and the Homebrew installation
  context. Hidden `__raw-log-writer` internals must not be presented as a public
  command. The option names and semantics must match `src/cli.rs` and the
  README; do not duplicate a stale or invented interface.
- In `.github/workflows/release.yml`, add `docs/stay.1` to each of the four
  target archives at a stable top-level archive path named `stay.1`. Update the
  package step and the final release-asset validation so every archive is
  required to contain exactly `LICENSE`, `README.md`, `stay`, and `stay.1` at
  its top level, with the executable mode retained and the man page regular and
  non-executable. The four target mappings, native builds, checksum manifest,
  and existing release gates must remain unchanged.
- Bump the application package version exactly once from `0.0.87` to `0.0.88` in
  `Cargo.toml` and `Cargo.lock` so the new archive is published under a new
  stable release. Do not change application source behavior or add a source
  build path. No Rust source version bump is otherwise implied by this
  documentation and packaging task.
- In `README.md`, retain the existing Homebrew, Cargo, Nix, runtime, and shell
  integration documentation and add a concise statement that the Homebrew
  install supplies the `stay(1)` man page, including the command users can run
  to read it.
- In the separate `nevdelap/homebrew-stay` repository, update only the tap
  formula and its existing formula test as needed for this deliverable. Point
  all four platform branches at the `v0.0.88` target-native release archives
  with the exact SHA-256 values from that release's `SHA256SUMS`; retain the
  existing target mapping, `tmux` dependency, tmux 3.6-or-newer check, and
  binary installation behavior. Add `man1.install "stay.1"` so the archive's
  top-level man page is installed into Homebrew's managed section-1 directory.
  Do not fetch a second copy from an unversioned URL, embed a generated page in
  the formula, hard-code a Homebrew prefix, or add Rust/Cargo/compiler
  dependencies.
- Extend the formula's existing `test do` block to assert that the installed
  `stay.1` exists below the formula's `man1` destination and is readable, in
  addition to preserving the installed version, JSON inventory, session
  lifecycle, tmux minimum, cleanup, and no-source-build assertions. The test
  must exercise the installed formula, not the application checkout. Make this
  an explicit Homebrew acceptance test: resolve the page with `man -w stay`,
  render it with `MANPAGER=cat man stay`, and assert that the rendered output
  contains the expected `stay(1)` identity and a stable documented synopsis or
  heading. The assertion must fail if the page is present only in the source
  checkout or release archive but is not installed and discoverable through
  Homebrew's normal manpath.
- Add man-page-specific project recipes `just format-man` and `just lint-man`,
  using mandoc `1.14.6` built from the official
  `https://mandoc.bsd.lv/snapshots/mandoc-1.14.6.tar.gz` source tarball and
  verified against SHA-256
  `8bf0d570f01e70a6e124884088870cbed7537f36328d512909eb10cd53179d9c`. Add
  `scripts/manpage-quality.sh` as the checked-in wrapper. It must download and
  verify that tarball, run its `./configure && make` build once in a task cache,
  verify that the resulting binary reports `1.14.6`, and fail if the version or
  checksum differs; do not use an unpinned host formatter. `just format-man`
  must run `mandoc -T utf8 -O width=80 -W warning docs/stay.1`, write the
  deterministic rendered preview to an ignored `target/man/stay.1.txt`, and fail
  on any warning, error, empty output, or formatting failure. `just lint-man`
  must run `mandoc -T lint docs/stay.1`; its diagnostics must remain visible and
  any non-zero exit must fail the recipe. Wire `format-man` into `just format`
  and `lint-man` into `just lint` whenever `docs/stay.1` is present, so
  `just qformat`, `just qlint`, `just qcheck`, and the exact remote
  `just mac-qcheck` inherit the same checks on Linux, macOS, and CI. Keep the
  source page hand-written and reviewable.
- Keep the application release and tap changes as separate coordinated
  deliverables. Nev must record the application commit SHA, tag and release URL,
  all four archive URLs and hashes, tap commit SHA, pull-request URL, and all
  gate results in the handoff. The application repository task commit must
  contain only the application man page, README, release workflow, package
  metadata, and man-page quality tooling; tap files belong only to the tap
  repository commit.

Acceptance criteria:

- `docs/stay.1` is a valid section-1 manual whose synopsis, public commands,
  options, configuration, logging, picker, shell integration, tmux requirement,
  and error behavior match the implemented CLI and README. It contains no hidden
  internal command documentation or source-build instructions.
- A tag-triggered application release at `v0.0.88` builds the same four native
  targets as the existing workflow, and every published archive contains exactly
  the four top-level entries `LICENSE`, `README.md`, `stay`, and `stay.1`.
  `stay` remains executable, `stay.1` is non-executable and readable, and all
  existing binary version and tmux smoke tests still pass.
- The release publishes exactly those four archives plus `SHA256SUMS`; the
  manifest has exactly four archive lines and the tap formula uses each matching
  literal release hash and target URL. No archive is selected by the wrong
  operating system or CPU architecture.
- On macOS Apple Silicon, macOS Intel, Linux ARM64, and Linux x86_64, a clean
  `brew tap nevdelap/stay` followed by `brew install nevdelap/stay/stay`
  installs both `stay` and the `stay(1)` man page without Rust, Cargo, a
  compiler, a source checkout, a manual copy, a symlink, a PATH edit, or a
  custom Homebrew prefix. The formula's Homebrew acceptance test proves this by
  making `man -w stay` resolve the installed page and by verifying stable
  content from `MANPAGER=cat man stay`, including the `stay(1)` identity and a
  documented synopsis or heading.
- The tap formula's existing audit, style, install, checksum, runtime tmux,
  version, JSON lifecycle, cleanup, and `brew test` checks pass on all four
  exact host and architecture combinations, including the new installed-man-
  page assertion. The formula continues to enforce tmux 3.6 or newer.
- The README preserves the existing installation instructions and accurately
  tells users that Homebrew installs the manual page and how to read it.
- The package version and lockfile agree at `0.0.88`, no application source
  behavior changes, and no unrelated files or release assets are changed.
- The application repository's final diff runs the exact applicable quiet gates
  `just qcheck`, `just mac-qcheck`, and `just qlint`, including the man-page
  formatting and linting recipes with mandoc 1.14.6; the separate tap repository
  runs its four-platform Homebrew audit/style/install/test/checksum gates. The
  handoff contains the application and tap commit SHAs, release and pull-request
  URLs, four archive hashes, archive listings and modes, and all gate results.
  Documentation, archive contents, and formula assertions are checked against
  the final release snapshot, not an earlier tag.

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
