# Review: TASK-111

## Findings

### R001

Status: ADDRESSED

The revised plan defines one canonical application commit and one separately
reviewed `TASK-111:` tap commit, keeps their file scopes separate, and explicitly
defines the `IMPLEMENTED` release boundary, Nev-owned tag/release/tap/PR steps,
required returned evidence, and the final Rufus review before `COMPLETED`. It
also states that an incomplete external release leaves the task `IMPLEMENTED`.
This is sufficient for the repository's coordinated handoff while preserving
the single shared commit contract for the application repository.

### R002

Status: ADDRESSED

The revised scope pins mandoc 1.14.6 and its official tarball and SHA-256,
requires a checked-in wrapper with version verification and cached build
behavior, specifies the exact `format-man` and `lint-man` commands and output
failure conditions, and wires them into the local, CI, and exact macOS quiet
gate paths. The man-page source remains hand-written and the verification
contract is now fully determined.

### R003

Status: ADDRESSED

The operator directed that mac-qcheck remain Rust-only because the configured
macOS host has no Docker. The plan and recipe now keep the man-page checks in
Linux `qcheck`/`qlint`; the exact macOS Rust gate passes.

### R004

Status: ADDRESSED

The canonical source now matches the approved behavior: failed attaches only
set the displayed error, while successful attaches persist the selection
(`src/picker/mod.rs:284-300`). The successful-selection behavior is inherited
from TASK-110 commit `ee6416e`; TASK-111 adds no picker Rust behavior or test
requirement.

### R005

Status: ADDRESSED

The plan now correctly records that mandoc 1.14.6 has no runtime version flag
and requires the feasible provenance checks instead: the extracted source
`Makefile` version and cached binary checksum. The wrapper implements those
checks (`scripts/manpage-quality.sh:32-95`), and direct checks confirmed that
`-V`, `-v`, and `--version` are unsupported upstream.

### R006

Status: ADDRESSED

The manual now documents the public top-level `--help`/`-h` and
`--version`/`-V` options (`docs/stay.1:70-76`), matching the Clap parser.

### R007

Status: ADDRESSED

The manual now documents explicit attach/create-attach status propagation and
signal-derived `128+N` statuses (`docs/stay.1:307-330`), matching `main` and
the relay implementation.

### R008

Status: ADDRESSED

The synopsis now uses escaped non-breaking separators for option arguments and
the default detach key uses the correct roff escape (`docs/stay.1:19,27,272-
274`). The mandoc rendered preview now displays the expected option spacing and
`Ctrl+\\` text.

### R009

Status: ADDRESSED

The `--prompt-integration` entry now includes the `eval` usage, prompt segment,
and zsh `PROMPT_SUBST` requirement (`docs/stay.1:56-73`), consistent with the
README.

### R010

Status: ADDRESSED

The configuration section now documents the `history_lines` default and calls
the log interval a positive integer with its default (`docs/stay.1:267-287`),
matching `src/config.rs`.

### R011

Status: ADDRESSED

The page now includes a concise standalone `COPYRIGHT` section with Nev
Delap's 2026 copyright and an MIT-license pointer (`docs/stay.1:351-357`),
consistent with the repository license (`LICENSE:1-3`).

### R012

Status: OPEN

The approved handoff is not complete. The plan requires Nev to provide the
published `v0.0.88` release URL, all four archive URLs and hashes, archive mode
and content checks, the separate tap commit and pull-request URLs, and the
four-platform tap CI and `brew test` results (`design_docs/implementation_plan.md:45-55,
126-132`). None of that external release/tap evidence is present in the
current workspace, so the task cannot yet move to `COMPLETED`.

### R013

Status: ADDRESSED

The wrapper now requires the pinned Docker image and runs both formatter and
linter invocations through `docker run`, mounting the repository and cached
binary (`scripts/manpage-quality.sh:116-148`). Fresh-cache format and lint
checks passed, including the Dockerized build, with no host mandoc invocation.

### R014

Status: ADDRESSED

An earlier shared commit message omitted the required review-document path from
its `Reviewed:` bullets. The current canonical commit's reviewer section now
points every review bullet to `review_docs/TASK-111.md`, while preserving the
finding history and the implementer's section.

### R015

Status: ADDRESSED

R015 identified that the previous mac-qcheck scope tried to run Dockerized
man-page checks on a macOS host without Docker. The operator explicitly
directed that mac-qcheck remain Rust-only; the plan and recipe now keep the
man-page checks in Linux `qcheck`/`qlint`, and the exact macOS gate passes.

### R015

Status: OPEN

The exact `just mac-qcheck` gate fails on the configured macOS host because
Docker is unavailable there. The remote command reaches `format-man`, whose
Docker requirement exits with `Docker is required to run pinned mandoc 1.14.6`.
The mandated macOS gate therefore cannot complete while format and lint are
required to run in Docker; Docker must be made available on that host or the
approved gate environment must be changed before completion.

## Implementation review notes

The current branch contains one TASK-111 implementation commit after the
planning commit; earlier intermediate TASK-111 commits are no longer part of
the branch history. TASK-111 adds no Rust source, test, or fixture changes; the
successful picker-selection behavior is from TASK-110. On the latest content,
`just qlint`, `just qcheck`, and the exact `just mac-qcheck` gates passed, as did
the fresh-cache Docker format/lint checks. R012 remains open.

## Final decision

Status: REVIEWED_FOUND_ISSUES
