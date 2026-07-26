# Review: TASK-022

## Findings

### R001

Status: ADDRESSED

The relay now captures attach start time, polls pane state on a 500 ms
cadence, detaches when a pane dies during the attach, and computes the final
status from pane-death time versus attach start
(`src/relay.rs:53-84`, `src/relay.rs:110-209`). Already-dead sessions remain
attached for postmortem review, while manual and signal detach paths use the
same attach-time status rule. The parser and status boundary have unit
coverage, and the real-PTY attachment tests cover automatic detach,
postmortem manual detach, and the command-end/manual-detach race
(`tests/attachment.rs:1195-1315`).

## Final decision

Status: COMPLETED

The complete current TASK-022 diff satisfies the implementation plan and
acceptance criteria. Independent verification passed: `just qcheck` and the
exact `just mac-qcheck` recipe both completed successfully.
