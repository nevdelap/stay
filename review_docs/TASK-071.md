# Review: TASK-071

## Findings

### R001

Status: ADDRESSED

Partial captures now keep `marker_bytes=0` when no marker was needed,
track marker progress only when marker bytes are actually part of the
payload, and apply the history-shift check to partial anchors. An unsafe
retry therefore emits the remaining/full marker as required, while a
safe retry still resumes after exactly the durable bytes. The added
`partial_append_followed_by_lost_overlap_emits_the_eviction_marker`
regression and the complete logging unit suite pass.

## Final decision

Status: COMPLETED
