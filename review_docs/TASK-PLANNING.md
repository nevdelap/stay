# Review: TASK-PLANNING

## Findings

### R001

Status: ADDRESSED

The workflow now distinguishes implementation and planning review completion:
planning approval leaves the task `NEW` and records `PLANNING_APPROVED`, while
only a clean implementation review sets the plan to `COMPLETED`
(agent_workflow.md:472-492).

### R002

Status: ADDRESSED

Scope now names the exact NixOS 26.05 and Home Manager 26.05 revisions and
requires the Home Manager nixpkgs input to follow the flake's locked node
(implementation_plan.md:36-48). The pinned commit references were also
verified against the corresponding upstream GitHub commit pages.

### R003

Status: ADDRESSED

Scope now specifies `nixosModules.stay`, `homeManagerModules.stay`, the exact
legacy attributes `stay`, `nixosModule`, and `homeManagerModule`, the module
arguments `{ pkgs, stay }`, and explicit package overrides
(implementation_plan.md:44-49, 175-176).

### R004

Status: ADDRESSED

Scope now requires `propagatedBuildInputs = [ pkgs.tmux ]`, fixes the module
dependency to exactly `pkgs.tmux`, and specifies the complete enabled,
disabled, and `enableTmux = false` matrix for both modules
(implementation_plan.md:81-105).

### R005

Status: ADDRESSED

Scope now specifies the exact `SHA256SUMS` URL, four-line/hash assertions,
named flake checks, module contexts, four-system matrix, and native-versus-
evaluation split (implementation_plan.md:61-64, 107-135, 182-186).

### R006

Status: ADDRESSED

TASK-108 now explicitly forbids modifying non-test `src/` and Cargo metadata,
keeps all Nix assertions at `v0.0.86`, and the workflow version rule exempts
packaging/documentation/workflow changes from a bump
(implementation_plan.md:27-32; agent_workflow.md:392-416).

### R007

Status: ADDRESSED

Scope now names exactly `README.md` and `.github/workflows/ci.yml`, and forbids
adding another workflow file (implementation_plan.md:141-153).

### R008

Status: ADDRESSED

The unmatched closing fences were removed and the documentation/workflow gate
passes on the corrected snapshot (agent_workflow.md:452-492).

### R009

Status: ADDRESSED

The amended planning commit deleted the tracked `review_docs/TASK-PLANNING.md`
from its predecessor, leaving the previous review only as an untracked working
tree file. This violated the rule to update one review document without losing
history. This pass restores the document to the shared commit and preserves all
prior findings and evidence.

### R010

Status: ADDRESSED

TASK-109 now separates the operator prerequisite from planned-task
dependencies and specifies the exact ref, expected commit, resulting
main-line baseline, and safe behavior for an absent or changed ref
(implementation_plan.md:22-35). It authorizes only deletion when the ref still
resolves to `d41e0a51013a24261292e4065cab2f8fef784460` and requires Nev to stop
if it resolves elsewhere.

### R011

Status: ADDRESSED

TASK-109 now explicitly changes `.github/dependabot.yml`, leaves the Cargo
entry unchanged, and requires exactly the `ignore` block for
`dtolnay/rust-toolchain` under the GitHub Actions entry, with no other ignore
rules (implementation_plan.md:67-78, 105-107).

### R012

Status: ADDRESSED

TASK-109 now requires the exact all-target test command and a separate exact
`--doc` command, explicitly explains their coverage, and makes either command
failure fail `just msrv` (implementation_plan.md:39-50, 85-96). The separate
command is valid for the repository's library target and was run successfully
with Rust 1.88.0 during this review.

### R013

Status: ADDRESSED

The planning commit refreshes TASK-108 from the superseded `v0.0.86` release
data to the operator-confirmed published `v0.0.88` release. It changes only
`design_docs/implementation_plan.md`, updates the manifest URL, all four
target-native archive URLs, literal SHA-256 values, SRI values, and every
`0.0.86` version assertion together, and leaves TASK-108 `NEW`. The parent
housekeeping handoff records the matching release hashes and archive evidence,
so the refreshed implementation scope is self-contained and ready for Igor.

### R014

Status: ADDRESSED

The commit subject identifies a planning commit, but its message omits the
mandatory `Implemented:` and `Reviewed:` sections and the required
`Co-Authored-By:` model trailer. It therefore does not satisfy the planning
commit contract or provide a pending review item for this task. Amend the
shared commit with the canonical sections, a review-document reference, and
the actual model trailer before approval. The reviewer amended the shared
commit accordingly.

### R015

Status: ADDRESSED

This entry reuses `TASK-109`, which was previously published for the completed
"split Rust toolchains and test the MSRV" task and whose review history was
later retired. The workflow explicitly forbids reusing a published task ID
after tasks are reordered or removed. The entry is now named TASK-112, which
is unused in the repository history, and its references are updated.

### R016

Status: ADDRESSED

The task's verification scope is not self-contained: its final criterion asks
for "applicable Dockerized Nix flake checks," but the repository's existing CI
uses native `nix build` followed by `nix flake check --system <system>` and no
Dockerized Nix check is defined. The task must name the exact package and
flake-check commands, all four system values, and the required `just qlint`
gate so Igor has executable evidence requirements rather than an undefined
verification path. TASK-112 now specifies the native `nix build` and
`nix flake check --system` commands and the four allowed `SYSTEM` values.

### R017

Status: ADDRESSED

The new task now states that native CI is authoritative and explicitly says it
does not add or depend on a Dockerized Nix environment. Its verification block
names the native commands and all four matrix systems.

## Verification

- `just qlint` passed on the planning commit; this ran the repository's
  formatting/linting path and its Dockerized gitlint check.
- `scripts/quality.py commit-message` passed with “commit message already
  formatted”.
- Direct host `gitlint --commit HEAD` was unavailable, but the repository gate
  above ran the configured gitlint container successfully.
- No test or fixture files changed in this planning commit.

## Final decision

Status: PLANNING_APPROVED

TASK-108's original planning review and R013 remain approved. R014-R017 are
addressed, and TASK-112 is approved for implementation while remaining `NEW`.
