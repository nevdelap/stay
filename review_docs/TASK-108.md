# Review: TASK-108

## Findings

### R001

Status: ADDRESSED

The documented flake installation commands point at
`github:nevdelap/stay/v0.0.88` (`README.md:41-42,49`). The existing
`v0.0.88` tag resolves to commit `2348ed785d728f5fb9e225d46cb07cf019219e2a`,
which has no `flake.nix` (`git cat-file -e v0.0.88:flake.nix` fails). Therefore
`nix run`, `nix profile install`, and the documented NixOS flake input fail to
find the flake that this commit adds. The README now uses
`github:nevdelap/stay`, which resolves the repository's current flake after
merge. Verified in the amended commit `0048ac2`.

### R002

Status: ADDRESSED

The Home Manager checks do not evaluate Home Manager. `flake.nix:91-100`
defines a hand-written `home.packages` option and `moduleConfig` only calls
`lib.evalModules`; `home-manager-flake`, `home-manager-legacy`, and
`home-manager-embedded` all use that fake base. In particular,
`home-manager-embedded` (`flake.nix:166-170`) is exactly the same four values
as `home-manager-flake`, and the pinned `home-manager` input is never used in a
check. No check loads `home-manager.lib.homeManagerConfiguration` or
`home-manager.nixosModules.home-manager`, so standalone Home Manager and
Home Manager embedded in NixOS are both unverified and the required embedded
check is now implemented with the pinned Home Manager API and
`home-manager.nixosModules.home-manager`; the build-enabled Linux flake check
realized the standalone activation and embedded configurations. Verified in
the amended commit `0048ac2`.

### R003

Status: ADDRESSED

The checks named `nixos-legacy` and `home-manager-legacy` bypass the legacy
entrypoint. `flake.nix:101-104` constructs their modules by importing the two
module files directly, then evaluates them through the same synthetic module
harness as the flake checks. Nothing in the checks imports `nix/default.nix`,
verifies its exact `stay`/`nixosModule`/`homeManagerModule` interface, or
exercises the `-I nixpkgs=...` and `--arg pkgs ...` invocation forms required
by the task. The checks now import `nix/default.nix`, assert its exact exported
attributes, and use its module exports; the legacy function signature and the
documented `nix-build`/`nix-env` forms remain supported. Verified in the
amended commit `0048ac2`.

### R004

Status: ADDRESSED

The standalone and embedded flake-based Home Manager examples use
`inputs.stay` and `inputs.home-manager` (`README.md:73-95`), but their module
argument lists do not bind `inputs`, nor do they show an
`extraSpecialArgs = { inherit inputs; }` setup. Copying either snippet as
shown produces an undefined-variable evaluation error. This fails the
acceptance requirement for complete, executable Home Manager commands. The
amended README binds `inputs` in both module snippets and shows the required
`extraSpecialArgs`/`specialArgs` setup. Verified in `0048ac2`.

### R005

Status: ADDRESSED

The task scope says the Nix-focused documentation belongs in exactly
`README.md` and names the repository files to add. The commit additionally
adds `docker/nix/Dockerfile` and modifies `docs/development.md` to document a
Dockerized Nix workflow. Those files are not in TASK-108's approved Scope;
the task commit must not include this separately scoped development workflow.
The amended commit removes both files and their documentation. Verified in
`0048ac2`.

### R006

Status: ADDRESSED

The `stay-package` check does not assert the required tmux runtime closure.
`flake.nix:54-66` verifies the output path, executable, version, and some
output-name exclusions, but never inspects propagated references or
`propagatedBuildInputs`. The package currently declares
`propagatedBuildInputs = [ pkgs.tmux ]` in `nix/package.nix:44`, but removing
that declaration would still leave `stay-package` green because the module
checks independently add `pkgs.tmux`. The task explicitly requires the named
package check to assert the propagated tmux closure. The amended
`stay-package` check asserts that `pkgs.tmux` is in
`stay.propagatedBuildInputs`. Verified in `0048ac2`.

### R007

Status: ADDRESSED

The amended `README.md` removes the existing checkout installation section
(`git clone https://github.com/nevdelap/stay.git`, followed by
`cargo install --path .`). TASK-108 explicitly requires existing Cargo and
installation documentation to remain intact. Users following the documented
source-checkout workflow have lost that installation path; restore the section
while retaining the Nix additions. The operator explicitly authorized this
documentation removal, and the task Scope and acceptance criteria now record
that exception. Verified in the amended commit `d75b125`.

### R008

Status: ADDRESSED

The standalone flake-based Home Manager example is still not executable as
documented. The `home.nix` snippet (`README.md:78-88`) only imports the Stay
module, while the `homeConfigurations.alice` snippet (`README.md:111-117`)
does not define `home.stateVersion`, `home.username`, or
`home.homeDirectory`. Evaluating the same minimal configuration against the
pinned Home Manager fails with `The option home.stateVersion was accessed but
has no value defined`. The flake check avoids this by injecting those options
at `flake.nix:118-122`, which the user-facing example does not do. Add the
required Home Manager settings to the documented standalone configuration.
The amended README now defines `home.username`, `home.homeDirectory`, and
`home.stateVersion`; the direct pinned Home Manager evaluation succeeds.
Verified in the amended commit `bb0bfb6`.

## Verification

- `just qlint` passed and left the worktree clean.
- `uv run --script scripts/quality.py commit-message` passed.
- The published `SHA256SUMS` manifest matched the four task hashes and its
  checked digest.
- The Linux release archive was inspected; its ELF dependencies are
  `libgcc_s`, `libm`, and `libc`, matching the package's Linux runtime inputs.
- In the repository's Dockerized Nix environment,
  `nix flake check --no-build --system x86_64-linux` passed evaluation.
- In that environment,
  `nix flake check --system x86_64-linux` passed the package build and all 37
  Linux checks, including Home Manager activation derivations.
- A direct pinned Home Manager evaluation of the documented minimal standalone
  configuration now succeeds and produces a Home Manager generation
  derivation.
- The legacy entrypoint manually evaluated to exactly
  `[ "homeManagerModule" "nixosModule" "stay" ]` with pinned nixpkgs.

## Final decision

Status: COMPLETED
