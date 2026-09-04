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
- Use one predictable Frizbee matching model per query rather than combining
  independent matchers, correcting queries, or merging result sets. Very short
  queries use Frizbee's literal substring mode; longer queries use its single
  Smith-Waterman fuzzy mode.

Dependencies:

- None.

Design decision:

- Choose `frizbee` 0.11.0 because its `Matcher` supplies the two modes needed by
  this one simple filter: literal substring matching for very short queries and
  Smith-Waterman scoring for ordered abbreviations and small typos. The fuzzy
  mode scores the complete ordered alignment, including skipped candidate
  characters and inserted, deleted, or substituted characters, so those
  behaviors participate in one ranking model. The package's documented MIT
  license and Rust 1.88-compatible release are compatible with the project.
- Reject `fuzzengine` 0.2.1 after testing its `partial_ratio` behavior against
  realistic picker abbreviations. It scores the best local candidate window, so
  changing its threshold cannot fix ranking inversions caused by candidate
  length and local alignment. In the observed cases, `strvw` ranked `staywtc`
  above `stayreview`, and `stwt` ranked `stayreview` above `staywtc`; both
  contradict the intended ordered abbreviation behavior. This is a scoring model
  failure, not merely a missed typo threshold.
- Use a minimum normalized match score of `0.70`. The picker normally shows only
  a few tens of sessions, so recall is more useful than suppressing every weak
  candidate: the intended match ranks first and the user can move once when an
  extra candidate is shown.
- Do not retain the current Nucleo matcher: its subsequence model permits
  skipped characters but cannot represent the typo behavior requested here.
- Do not choose `fuzzy-regex`: it supports edit-tolerant literals, including
  Damerau-Levenshtein-style matching, but introduces a full regex language and
  engine for a plain session-name search.
- Do not choose `textdistance`, `fuzzt`, or another distance-metrics crate: they
  provide distance functions but still require Stay to implement the search,
  thresholding, and ranking model that `frizbee` already provides. Do not choose
  `sublime_fuzzy`, since its scoring remains an arbitrary-gap character matcher
  and does not add typo tolerance. `fuzzy-matcher` has the same missing
  typo-tolerance property, while `fuzzy-regex` introduces a full regex engine
  for a plain session-name search.
- This is a single matcher and score, not a Nucleo-plus-edit-distance fallback,
  a spelling-correction pass, or a merged result strategy. The short-query
  literal mode is a mode selection on the same Frizbee matcher, not a second
  pass or result set. The lowercase normalization described below is input
  preparation, not a second matcher.

Scope:

- Linux and macOS interactive picker sessions in both the normal terminal
  display path and the alternate-screen path. All picker entry paths that use
  `/` filtering are included; non-picker CLI listing, pass-through, create,
  edit, attach, and installation/package variants are not changed.
- Replace the Nucleo subsequence matcher in `src/picker/mod.rs` with one
  `frizbee` Smith-Waterman matcher per query. Preserve the existing managed
  worker, inventory snapshot, cancellation, generation checks, and name-to-index
  resolution. Remove the `nucleo` dependency if the implementation no longer
  uses it, and add the pinned `frizbee` dependency to `Cargo.toml` and
  `Cargo.lock`.
- Define matching as case-insensitive ordered fuzzy matching for a normalized
  query of at least three Unicode characters: candidate characters may be
  skipped without a fixed limit for abbreviations, and Frizbee may align
  inserted, deleted, or substituted characters for typo tolerance. Configure
  `max_typos = Some(2)` only for its documented meaning: at most two characters
  missing from the needle may pass Frizbee's typo-aware prefilter. It is not a
  promise of two total edit operations; skipped candidate characters remain
  unlimited and are penalized by the alignment score, while substitutions and
  other alignment costs are bounded by the normalized score threshold.
- Use `CaseMatching::Ignore` and `UnicodeMatching::Always` in Frizbee. Lowercase
  both strings with Rust's Unicode `char::to_lowercase` mapping before matching,
  preserve all remaining Unicode characters and whitespace, and do not
  transliterate, accent-fold, or compatibility-normalize. Matching keys are
  temporary and the original names remain the display and result values.
- Use a custom Frizbee `Scoring` value with `match_score` and the default gap
  and mismatch penalties, but all bonuses set to zero: `prefix_bonus`,
  `capitalization_bonus`, `matching_case_bonus`, `exact_match_bonus`, and
  `delimiter_bonus`. For fuzzy queries, normalize the raw score as
  `score / (normalized_query_character_count * match_score)` and accept scores
  at `0.70` or above. This denominator is bounded by the actual configured
  scoring model and cannot exceed one due to a bonus. Rank accepted names by raw
  Frizbee score descending, with original inventory order as the stable
  tie-breaker. An empty query continues to return the inventory in its original
  order.
- For one- and two-character normalized queries, select Frizbee's
  `Matching::Substring` mode with the same case and zero-bonus scoring
  configuration. It accepts only an exact contiguous case-insensitive substring,
  ignores typo settings as required by Frizbee, gives each accepted name the
  same raw score, and therefore preserves inventory order. This is the only
  short-query exception and has no broad typo matching.
- This intentionally keeps one simple matcher and score. It does not combine
  separate abbreviation and typo matchers, correct queries, or merge result
  lists. The filter documentation must describe ordered fuzzy matching with
  small spelling-error tolerance.
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
  state that filtering is case-insensitive ordered fuzzy matching, tolerates
  small spelling errors, and allows skipped candidate characters for
  abbreviations, including the documented `realese`/`release` example.
- Because application source under `src/` changes, increment the package patch
  version exactly once from the task baseline and update `Cargo.lock` and every
  version assertion together.

Acceptance criteria:

- The picker uses one Frizbee matcher per query, with its documented literal
  substring mode for one- and two-character queries and its Smith-Waterman fuzzy
  mode for longer queries. It has no Nucleo pass, edit-distance fallback,
  spelling correction, or merged result lists.
- In the interactive picker, `realese` finds and selects a session named
  `release`, while ordinary exact/partial matches, case-insensitive matching,
  Unicode names, empty queries, stable ties, and the existing worker
  generation/cancellation behavior continue to work.
- One- and two-character queries accept only exact contiguous case-insensitive
  substrings, preserve inventory order, and do not gain typo matching.
  Longer-query names below the normalized `0.70` score are not shown.
- The README and manual page accurately document case-insensitive ordered fuzzy
  matching, skipped candidate characters for abbreviations, and small spelling
  errors.
- The Linux acceptance run verifies typo filtering and attachment in both the
  normal main-screen and alternate-screen picker paths.
- The macOS acceptance run verifies the same two picker paths on the configured
  macOS host; failure to provide that evidence leaves the task incomplete.
- The final diff passes the exact `just qcheck`, `just mac-qcheck`,
  `just qacceptance`, and `just mac-qacceptance` recipes.
