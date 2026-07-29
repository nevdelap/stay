# Review: TASK-042

## Findings

### R001

Status: ADDRESSED

TASK-042 requires live integration coverage for both alternate-screen and
forced-main-screen attachment preferences. The new
`picker_attachment_status_covers_auto_and_forced_main_screen` test now
exercises Stay's default `Auto` path and its `--no-alt-screen` forced-main
path. The focused test passes, and the existing Linux test continues to cover
all four client-flag combinations.

## Final decision

Status: COMPLETED
