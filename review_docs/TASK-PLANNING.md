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

TASK-108's earlier planning review remains approved. TASK-109 now meets the
quality bar, leaves no substantive design decision for Igor, remains `NEW`, and
is ready for implementation.
