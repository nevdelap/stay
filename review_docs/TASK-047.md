# Review: TASK-047

## Findings

No material findings.

The picker title is derived from `env!("CARGO_PKG_VERSION")`, contributes its
display width to picker sizing, and is truncated to the available inner width
without disturbing narrow-frame borders. The render tests cover the exact
package-version title and a narrow frame. The required verification gates pass.

## Final decision

Status: COMPLETED
