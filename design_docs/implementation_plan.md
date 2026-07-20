# Implementation Plan

This file is the task source of truth for orchestrated `orc` work.

Before starting the orchestrator for a new change, add one `NEW` task
under `Tasks`. Task scoping, the task template, the commit contract,
and the task-state rules live in `design_docs/team_specification.md`.

## Tasks

The first tasks below move the current supervisor-driven workflow toward
the orchestrator-owned state machine and watchdog model described in
`docs/architecture.html`.
