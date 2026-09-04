# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-113 - typo-tolerant picker filtering

State: NEW

Goal:

- Make the interactive picker find a session when the filter query contains a
  small spelling error, such as finding `release` for the query `realese`.
- Use one predictable approximate-substring matching model rather than combining
  independent matchers, correcting queries, or merging result sets.

Dependencies:

- None.

Design decision:

- Choose `fuzzengine` 0.2.1 because its `partial_ratio` API supplies one
  approximate-substring score for the best matching part of a candidate. It
  handles insertions, deletions, and substitutions without requiring Stay to
  implement an edit-distance algorithm, and its linear scan is more than fast
  enough for the expected few tens of sessions. The package's documented MIT
  license and pure-Rust implementation are compatible with the project.
- Do not retain the current Nucleo matcher: its subsequence model permits
  skipped characters but cannot represent the typo behavior requested here.
- Do not choose `fuzzy-regex`: it supports edit-tolerant literals, including
  Damerau-Levenshtein-style matching, but introduces a full regex language and
  engine for a plain session-name search.
- Do not choose `textdistance`, `fuzzt`, or another distance-metrics crate: they
  provide distance functions but still require Stay to implement the substring
  search, thresholding, and ranking model that `fuzzengine` already provides. Do
  not choose `sublime_fuzzy`, since its scoring remains an arbitrary-gap
  character matcher and does not add typo tolerance.
- This is a single matcher and score, not a Nucleo-plus-edit-distance fallback,
  a spelling-correction pass, or a merged result strategy. The lowercase
  normalization described below is input preparation, not a second matcher.

Scope:

- Linux and macOS interactive picker sessions in both the normal terminal
  display path and the alternate-screen path. All picker entry paths that use
  `/` filtering are included; non-picker CLI listing, pass-through, create,
  edit, attach, and installation/package variants are not changed.
- Replace the Nucleo subsequence matcher in `src/picker/mod.rs` with the
  `fuzzengine` approximate-substring matcher. Use its single partial-match score
  for each session name, preserving the existing managed worker, inventory
  snapshot, cancellation, generation checks, and name-to-index resolution.
  Remove the `nucleo` dependency if the implementation no longer uses it, and
  add the pinned `fuzzengine` dependency to `Cargo.toml` and `Cargo.lock`.
- Define matching as case-insensitive approximate substring matching: compare
  the query with the best matching contiguous substring of each session name.
  Before calling `fuzzengine`, lowercase both strings with Rust's Unicode
  `char::to_lowercase` mapping, configure `PreprocessingOptions` with ASCII
  folding disabled and stripping disabled, and preserve all remaining Unicode
  characters and whitespace. Do not transliterate, accent-fold, compatibility-
  normalize, or otherwise rewrite the original session name; matching keys are
  temporary and the original names remain the display and result values. Accept
  a non-empty query of at least three Unicode characters when the normalized
  partial score is at least `0.70`, and use exact case-insensitive substring
  matching for one- and two-character queries. Rank accepted names by score
  descending, with original inventory order as the stable tie-breaker. An empty
  query continues to return the inventory in its original order.
- Intentionally replace the current arbitrary-gap subsequence semantics; the
  filter documentation must describe approximate substring matching instead. The
  exact Unicode behavior is the lowercase-only, non-transliterating model
  defined above.
- Add deterministic matcher tests in `src/picker/mod.rs` covering exact and
  partial matches, `realese` matching `release`, missing/extra/replaced
  characters, rejection of clearly unrelated names, short-query behavior,
  case-insensitivity, lowercase-only Unicode normalization without
  transliteration, stable ties, and empty-query ordering.
- Extend the real-PTY picker coverage in `tests/attachment.rs` to type a typo
  query, wait for the observed filtered redraw, and verify that the intended
  session is selected/attachable. Keep the existing readiness checkpoints and
  exercise both terminal-screen modes when the shared test path supports them.
- Update the picker-filter descriptions in `README.md` and `docs/stay.1` to
  state that filtering is case-insensitive approximate substring matching and
  tolerates small spelling errors, including the documented `realese`/ `release`
  example.
- Because application source under `src/` changes, increment the package patch
  version exactly once from the task baseline and update `Cargo.lock` and every
  version assertion together.

Acceptance criteria:

- The picker uses one `fuzzengine` approximate-substring score and one
  threshold; it has no Nucleo pass, edit-distance fallback, spelling correction,
  or merged result lists.
- In the interactive picker, `realese` finds and selects a session named
  `release`, while ordinary exact/partial matches, case-insensitive matching,
  Unicode names, empty queries, stable ties, and the existing worker
  generation/cancellation behavior continue to work.
- One- and two-character queries do not gain broad typo matching, and names
  below the configured score threshold are not shown.
- The README and manual page accurately document the new matching semantics and
  the intentional loss of arbitrary-gap subsequence matching.
- The final diff passes `just qcheck`, `just mac-qcheck`, `just qacceptance`,
  and `just mac-qacceptance`.
