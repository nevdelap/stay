# Review: TASK-072

## Findings

### R001

Status: ADDRESSED

The raw stream now launches the hidden stay writer rather than a shell
redirection. That writer calls the shared validated `O_NOFOLLOW` append opener
before copying pane bytes, and the raw-writer symlink regression leaves the
replacement target untouched. The logging unit suite and all raw attachment
regressions pass.

## Final decision

Status: COMPLETED
