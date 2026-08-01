#!/usr/bin/env -S uv run --script
"""Summarize the current worktree for a developer or coding agent."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from scripts import quality


def git(*arguments: str) -> list[str]:
    result = subprocess.run(
        ["git", *arguments],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return [line for line in result.stdout.splitlines() if line]


def changed_paths() -> list[str]:
    tracked = git("diff", "--name-only", "--diff-filter=ACMR", "HEAD", "--")
    untracked = git("ls-files", "--others", "--exclude-standard")
    return sorted(set(tracked + untracked))


def print_test_guidance(paths: list[str]) -> None:
    targets = sorted(
        Path(path).stem
        for path in paths
        if path.startswith("tests/") and path.endswith(".rs")
    )
    if targets:
        print("Likely tests:")
        for target in targets:
            print(f"  just test-target {target}")
        return

    if any(
        path.endswith(".rs") or path in {"Cargo.toml", "Cargo.lock"} for path in paths
    ):
        print("Likely tests:")
        print("  just test-nextest")
    elif "scripts/test_quality.py" in paths:
        print("Likely tests:")
        print("  uv run --script scripts/test_quality.py")
    else:
        print("Likely tests: none inferred for this change")


def main() -> None:
    paths = changed_paths()
    if not paths:
        print("No uncommitted or untracked changes.")
        return

    print("Changed files:")
    for path in paths:
        print(f"  {path}")

    existing = [
        path
        for path in paths
        if (ROOT / path).is_file() and not (ROOT / path).is_symlink()
    ]
    groups = quality.classify(existing)
    active_groups = [name for name, members in groups.items() if members]
    print(f"Quality groups: {', '.join(active_groups) or 'none'}")
    print("Changed-file quality: just qformat && just qlint")
    print_test_guidance(paths)
    print("Fast gate: just qcheck-fast")
    print("Full handoff: just qcheck && just mac-qcheck")
    print(f"Failure log: {ROOT / 'check.log'}")


if __name__ == "__main__":
    main()
