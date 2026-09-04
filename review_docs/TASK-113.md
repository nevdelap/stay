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

## Final decision

Previous decision: `PLANNING_APPROVED` for the former fuzzengine plan. That
decision is superseded by the revised Frizbee plan.

Status: PLANNING_APPROVED

R001-R010 are addressed. TASK-113 remains `NEW` and is approved for Igor to
implement from this planning baseline.
