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

## Final decision

Status: PLANNING_APPROVED

R001 and R002 are addressed. TASK-113 is approved for implementation and
remains `NEW`.
