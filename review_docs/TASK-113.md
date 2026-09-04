# Review: TASK-113

## Findings

### R001

Status: ADDRESSED

The selected `fuzzengine` API does not provide the normalization semantics
that this task requires. In the selected 0.2.1 release,
`PreprocessingOptions` has only `force_ascii` and `strip`: the default
transliterates non-ASCII input through `any_ascii`, while disabling
`force_ascii` preserves Unicode but does not make comparisons
case-insensitive. There is no case-folding option. See the crate's
[`PreprocessingOptions`](https://docs.rs/fuzzengine/0.2.1/fuzzengine/struct.PreprocessingOptions.html)
and [`partial_ratio`](https://docs.rs/fuzzengine/0.2.1/fuzzengine/fn.partial_ratio.html)
APIs.

Therefore a direct `partial_ratio` integration cannot simultaneously satisfy
the plan's case-insensitive requirement and its promise to retain the current
Unicode-aware behavior (implementation_plan.md:61-71). The design decision
also does not say whether transliteration is intended, whether original names
remain the display values, or which exact Unicode normalization/case-folding
rules apply. This is material because it determines matching results and the
required dependency/API surface, not merely an implementation detail.

The plan now specifies the exact lowercase-only, non-transliterating
normalization model, disables `fuzzengine`'s ASCII folding and trimming,
preserves original names as display/result values, and requires deterministic
tests for the selected Unicode behavior (implementation_plan.md:55-82). The
`realese` example itself remains compatible with `partial_ratio`.

#### Resolution evidence

R001 is addressed for planning purposes. The implementation must follow the
newly specified normalization contract rather than relying on
`PreprocessingOptions::default()`.

### R002

Status: ADDRESSED

The planning task is marked `IMPLEMENTED` at implementation_plan.md:14. This
violates the workflow contract: a planning commit must not set its planned task
to `IMPLEMENTED`; that state is reserved for Igor's later implementation
commit containing the scoped code or tests. While a planning finding is open,
the reviewer must use `REVIEWED_FOUND_ISSUES`; after all planning findings are
resolved, the final planning decision must be `PLANNING_APPROVED` and the task
must remain `NEW` (agent_workflow.md:184-186, 472-492).

The plan now leaves TASK-113 in `NEW`, as required for a planned task
(implementation_plan.md:12-14).

#### Resolution evidence

R002 is addressed. The planning commit remains distinct from the future task
implementation commit, and TASK-113 is eligible for Igor without being marked
as implemented prematurely.

### R003

Status: ADDRESSED

The revised plan's single-matcher rule conflicts with its short-query rule
(implementation_plan.md:68-85 and :112-120). Frizbee's `Matching::Substring`
is a literal mode, not Smith-Waterman, and the package documents that literal
modes do not support typos. The plan nevertheless requires one Frizbee
Smith-Waterman matcher and one Smith-Waterman score while also requiring exact
case-insensitive substring matching for one- and two-character queries.
An implementation must either add a second matching mode/branch or add custom
logic around the Smith-Waterman result. Either choice changes the claimed
single-model design and leaves the score, threshold, and stable ordering for
short queries unspecified. The plan must choose one coherent model and state
how short queries are scored and filtered.

See Frizbee's [`Config` documentation](https://docs.rs/frizbee/0.11.0/frizbee/struct.Config.html),
which describes the matching modes and the literal-mode limitation.

#### Resolution evidence

R003 is addressed. The plan now defines one mode per query: literal substring
matching for one- and two-character queries, and Smith-Waterman matching for
longer queries. It also defines short-query scoring and states that the modes
do not produce separate result sets.

### R004

Status: ADDRESSED

The plan says that `max_typos = 2` permits up to two insertion, deletion, or
substitution operations (implementation_plan.md:74-80), but that is not what
Frizbee's option means. In 0.11.0, `Config::max_typos` is documented as the
maximum number of characters missing from the needle. Skipped candidate
characters are fuzzy gaps and are not bounded by that option; substitutions
and missing query characters are handled through the alignment-path typo
count. Thus the proposed configuration does not enforce two total edit
operations, and the plan does not say whether arbitrary candidate gaps are
intended. Define the exact typo budget and candidate-gap behavior before
implementation, including how the implementation will enforce it with the
chosen single matcher.

See the [`max_typos` field documentation](https://docs.rs/frizbee/0.11.0/frizbee/struct.Config.html#structfield.max_typos)
and Frizbee's [algorithm description](https://github.com/saghen/frizbee/blob/main/README.md#smith-waterman).

#### Resolution evidence

R004 is addressed. The plan now explicitly limits `max_typos` to Frizbee's
documented missing-needle-character prefilter semantics, states that skipped
candidate characters are unlimited and score-penalized, and removes the false
promise of two total edit operations.

### R005

Status: ADDRESSED

The normalized-score formula is not an ideal Frizbee score
(implementation_plan.md:77-80). It includes only `query character count ×
match_score + prefix_bonus`, while Frizbee's default Smith-Waterman scoring
also applies `matching_case_bonus` to case matches and can apply
`exact_match_bonus` to a whole-candidate exact match. The plan lowercases both
inputs but does not specify a custom scoring configuration, so the proposed
normalization can exceed 1.0 and varies depending on whether the candidate is
an exact match or receives other bonuses. It also does not define whether the
character count is measured before or after lowercase expansion. Specify a
complete, bounded normalization formula using the actual scoring config, or
replace it with a clearly defined raw-score threshold and ranking contract.

See Frizbee's [`Scoring` documentation](https://docs.rs/frizbee/0.11.0/frizbee/struct.Scoring.html).

#### Resolution evidence

R005 is addressed. The plan now sets every Frizbee bonus to zero and defines
the denominator from the normalized query character count and configured match
score, so the threshold is bounded by the stated scoring model.

### R006

Status: ADDRESSED

The Unicode contract is incomplete (implementation_plan.md:74-85). Frizbee's
default `UnicodeMatching::Smart` uses the byte-oriented path for an ASCII
needle even when a candidate contains multi-byte Unicode characters, which
changes gap penalties and therefore both thresholding and ranking. The plan
requires preserving Unicode and deterministic Unicode behavior but does not
select `UnicodeMatching::Always` or explicitly accept the Smart behavior. It
also does not select a Frizbee `CaseMatching` mode after performing its own
lowercasing. Specify both modes and add tests that cover an ASCII query against
a Unicode candidate, not only Unicode queries.

See Frizbee's [Unicode limitations](https://github.com/saghen/frizbee/blob/main/README.md#limitations)
and the [`UnicodeMatching` documentation](https://docs.rs/frizbee/0.11.0/frizbee/enum.UnicodeMatching.html).

#### Resolution evidence

R006 is addressed. The plan now selects `CaseMatching::Ignore` and
`UnicodeMatching::Always`, defines lowercase-only preparation, and requires
deterministic Unicode and case-insensitivity tests.

### R007

Status: ADDRESSED

The documentation requirement still calls the behavior “case-insensitive
approximate substring matching” (implementation_plan.md:102-105), while the
new design intentionally allows skipped candidate characters for ordered
abbreviations (implementation_plan.md:74-76 and :89-92). Approximate
substring matching implies a contiguous window and does not accurately
describe the selected Smith-Waterman model. The plan must use one consistent
user-facing term and require README and manual-page text that explains both
ordered gaps and typo tolerance.

#### Resolution evidence

R007 is addressed. The plan now consistently calls the behavior ordered fuzzy
matching and requires the README and manual page to describe skipped candidate
characters as well as typo tolerance.

### R008

Status: ADDRESSED

The scope names Linux and macOS plus normal and alternate terminal-screen
paths (implementation_plan.md:64-67), but the acceptance criteria do not
state the required behavior evidence for each platform and screen variant.
The PTY requirement only says to exercise both modes “when the shared test path
supports them” (implementation_plan.md:98-101), which leaves the required
coverage conditional and does not define how the macOS and Linux paths are
verified. Make the acceptance matrix explicit, or narrow the scope before
implementation.

#### Resolution evidence

R008 is addressed. The acceptance criteria now require the Linux and macOS
acceptance runs to verify both the main-screen and alternate-screen picker
paths, with missing macOS evidence explicitly preventing completion.

### R009

Status: ADDRESSED

The planning commit message has blank lines between items in both the
`Implemented:` list and the `Reviewed:` list. The commit contract requires no
blank lines between list items. The reviewer must not approve the planning
commit until the message is amended while preserving the implementer's
section.

#### Resolution evidence

R009 is addressed. The amended planning commit has no blank lines between
items in either the `Implemented:` or `Reviewed:` lists, while the review
section and the implementer's section remain intact.

### R010

Status: ADDRESSED

The task state is `NEW` while R009 remains open. The workflow requires
`REVIEWED_FOUND_ISSUES` whenever material planning findings remain; `NEW` is
the state used only after a planning review has reached
`PLANNING_APPROVED` (agent_workflow.md:399 and :484-492). Restore the task
state to `REVIEWED_FOUND_ISSUES` until the commit message is corrected and the
planning review is approved.

#### Resolution evidence

R010 is addressed. The plan's State is now
`REVIEWED_FOUND_ISSUES`, correctly recording that the planning review is
blocked by the open commit-message finding.

### R011

Status: ADDRESSED

The vendoring strategy is not sufficiently self-contained for the stated
MSRV fix (implementation_plan.md:38-47 and :90-94). In Frizbee 0.11.0,
AVX-512 is not a Cargo-optional backend: on x86_64 its modules are compiled
unconditionally, and Rust 1.88 rejects both the AVX-512 `target_feature`
attributes and the `stdarch_x86_avx512` intrinsics. The affected code is
spread across the literal, prefilter, matcher, and Smith-Waterman backends,
their dispatch/type definitions, and backend contract tests. Removing only
the target-feature declarations still leaves the unstable intrinsics and
does not produce a buildable crate.

Saying “remove the optional AVX-512 backend and its target-feature
declarations” therefore does not specify the coordinated source and test
surface that Igor must change. The plan must specify the exact disablement
strategy and affected modules (or include a checked-in patch with source
provenance), and require all-feature tests of the resulting vendored crate
under Rust 1.88 and the current toolchain. This is material because the
implementation cannot satisfy the stated MSRV and acceptance gates from the
current vendor instructions.

#### Evidence

An independent `cargo +1.88 check` against the unmodified crates.io
`frizbee` 0.11.0 source reproduced E0658 failures for AVX-512
`target_feature` attributes and `stdarch_x86_avx512` intrinsics. The source
also contains AVX-512 dispatch and parity-test references outside those
backend files.

#### Resolution evidence

R011 is addressed by the latest plan: it removes the vendoring and patching
strategy and instead raises the project MSRV to Rust 1.89, where the required
Frizbee AVX-512 features are available. The plan now requires the unmodified
crates.io dependency to pass the MSRV checks.

### R012

Status: ADDRESSED

The MSRV change does not define a consistent compiler-pinning policy
(implementation_plan.md:38-43, :85-89, and :161-165). The acceptance criterion
requires “the same compiler version” for the macOS check, CI, and release, but
the scope only says to update the existing 1.88 pins. In the current
repository, `mac-qcheck` invokes `scripts/maccmd.sh cargo test`, which uses the
tracked `rust-toolchain.toml` channel `stable`; CI's normal check, acceptance,
stable, and lint jobs also use floating `stable`, while only the CI MSRV job
and release binary build are explicitly pinned.

The plan must say which paths are intentionally MSRV-pinned to 1.89 and which
remain floating stable, then name the exact commands/configuration and update
the acceptance criterion accordingly. Alternatively, it must scope the
macOS/local and all relevant CI paths to an explicit 1.89 toolchain. As
written, Igor can update the listed 1.88 occurrences and still violate the
plan's “same compiler version” acceptance criterion, leaving the MSRV policy
and verification result ambiguous.

#### Evidence

`justfile:214-215` defines `mac-check` as
`scripts/maccmd.sh cargo test`; `rust-toolchain.toml` selects `stable`.
`.github/workflows/ci.yml` uses `stable` for the check, acceptance, stable,
and lint jobs, while its MSRV job is the explicit 1.88 pin. The release
workflow separately pins its binary build to 1.88.0.

#### Resolution evidence

R012 is addressed. The latest plan explicitly defines the policy: the MSRV
checks, CI MSRV job, and release binary builds use Rust 1.89, while ordinary
local checks, macOS checks, acceptance paths, and CI stable jobs intentionally
remain on floating `stable`. The acceptance criteria now require those exact
paths to follow that split rather than requiring one compiler version
everywhere.

### R013

Status: ADDRESSED

The new best-result fallback defeats the stated `0.70` minimum and can display
a weak candidate as if it were a valid match (implementation_plan.md:51-58,
:127-134, and :174-178). With Frizbee 0.11.0 configured with
`max_typos = Some(2)` and its default `match_score` of 16, an independent
match of `relxaz` against `release` returns raw score 65. The plan's formula
normalizes that to `65 / (6 * 16) = 0.677`, below the `0.70` threshold, but the
fallback would still show `release` when no other result reaches the
threshold.

This makes “minimum normalized match score” no longer true and weakens the
existing requirement to reject clearly unrelated names. It also changes the
meaning of the threshold from acceptance filtering to “show the closest
candidate,” which could select the wrong session in a small inventory. The
plan must either remove the fallback and keep `0.70` as a hard acceptance
threshold, or explicitly redefine the contract with a justified lower bound
and tests proving that weak/unrelated queries remain empty. The current
exception is not enough because any candidate admitted by Frizbee's typo
prefilter can bypass the threshold.

#### Evidence

An independent current-toolchain run against the unmodified crates.io
`frizbee` 0.11.0 source with `Config::default().max_typos(Some(2))` returned
`Match { score: 65, index: 0, exact: false }` for `relxaz`/`release`, with no
candidate meeting the plan's normalized `0.70` threshold.

#### Resolution evidence

R013 is addressed. The user clarified that `0.70` is the normal result-list
threshold, not a requirement that the filter return nothing when every
candidate is weak. The plan's fallback intentionally retains the single best
result in that case, while retaining all results at or above `0.70` and
leaving the list empty only when Frizbee returns no candidate. An independent
run with Frizbee 0.11.0 and `max_typos = Some(2)` returned `staydev` for
`sdv` (raw score 48), confirming the intended ordered abbreviation case.

## Final decision

Previous decision: `PLANNING_APPROVED` for the former fuzzengine plan. That
decision is superseded by the revised Frizbee plan.

Status: PLANNING_APPROVED

R001-R013 are addressed. TASK-113 remains `NEW` and is approved for Igor to
implement from this planning baseline.
