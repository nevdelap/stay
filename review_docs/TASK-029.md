# Review: TASK-029

## Findings

### R001

Status: ADDRESSED

`src/tmux.rs:447-452`, the doc comment on `attach_program_and_arguments`,
contained a broken sentence: "Neither flag is set is byte-identical to the
plain attach argv used before these modifiers existed." This did not parse
as English (subject/verb mismatch — "neither flag is set" is not a clause
that "is byte-identical" can attach to) and would confuse a reader trying to
learn the invariant from the doc comment alone.

Evidence of resolution: `src/tmux.rs:451-452` now reads "When neither flag
is set, the resulting argv is byte-identical to the plain attach argv used
before these modifiers existed." — the exact rewording suggested, applied
with no other change to the diff.

## Final decision

Status: COMPLETED

The implementation is correct and complete against the TASK-029 goal, scope,
and acceptance criteria:

- `Tmux::attach_program_and_arguments` composes `-f read-only` /
  `-f ignore-size` / `-f read-only,ignore-size` independently rather than
  tmux's bundled `-r`, and omits `-f` entirely when neither modifier is set
  (verified byte-identical to the pre-existing plain-attach argv by
  `relay_attach_argv_omits_flag_argument_when_no_modifier_is_set`).
- The modifiers are threaded end to end: `relay::attach`/`attach_with_input`,
  `session::attach_session`/`attach_session_with_input`, and the `attach`
  subcommand dispatch in `main.rs`, which also drops `-r`/`-L` from
  `reject_unimplemented_attach_options` now that they're implemented.
- The picker's `v`/`l` keys now produce real `PickerOutcome::Attach` variants
  (read-only / low-priority respectively) via the new shared
  `attach_outcome` helper, replacing the placeholder `action_error`s; Enter
  and the post-create attach path are unaffected (both pass
  `false, false`).
- Test coverage matches the task's Scope exactly: argv unit tests for all
  four flag combinations, a real-PTY integration test
  (`read_only_attach_does_not_forward_input_to_the_pane`) proving a read-only
  attach's keystrokes don't reach the pane and that detach still works, and
  picker unit tests for both the `v`/`l` outcome shape and the
  no-selection-is-a-no-op case.
- `design_docs/stay.html` strikes the TODO-002 index entry and both TODO-002
  body sections, and un-strikes the picker key list's `v`/`l` entries,
  matching the doc's existing convention.
- The version bump (`0.0.12` -> `0.0.13`) is the only `Cargo.lock` change;
  no new dependency was introduced.

Independent verification: two consecutive clean `just qcheck` runs (no
further file changes after either), and the exact `just mac-qcheck` recipe,
both passed, including the new tests
(`relay_attach_argv_maps_read_only_to_the_read_only_client_flag`,
`relay_attach_argv_maps_low_priority_to_the_ignore_size_client_flag`,
`relay_attach_argv_composes_both_client_flags_independently`,
`relay_attach_argv_omits_flag_argument_when_no_modifier_is_set`,
`view_only_and_low_priority_keys_produce_matching_attach_outcomes`,
`view_only_and_low_priority_keys_are_ignored_without_a_selection`,
`read_only_attach_does_not_forward_input_to_the_pane`) confirmed present and
passing in both `check.log` runs.

R001 is addressed and no other issues were found on this pass. Approved.
