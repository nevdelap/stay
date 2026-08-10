# Review: TASK-103

## Findings

### R001

Status: ADDRESSED

TASK-103 explicitly requires the shared helpers to be loaded once from
`setup_file()`. `tests/acceptance.bats:7-8` loads both helpers at file scope
instead, and the comment at `tests/acceptance.bats:5-6` documents that
deviation. Move the `load` calls into `setup_file()` while preserving the
single-load behavior, or update the task specification before approval.

The follow-up now loads both helpers from `setup_file()` at
`tests/acceptance.bats:9-11` and exports the required helper definitions to
the test-case shells through the generated `BASH_ENV` file. The exact Linux
and macOS Rust and acceptance gates pass.

## Verification

- Exact `just qcheck`: passed.
- Exact `just mac-qcheck`: passed.
- Exact `just qacceptance`: passed.
- Exact `just mac-qacceptance`: passed.
- `git diff --check HEAD^ HEAD`: passed before review metadata changes.
- `tests/acceptance.bats` contains no executable `tmux` invocation.
- The PTY helper exposes exactly the six specified public functions.

## Final decision

Status: COMPLETED
