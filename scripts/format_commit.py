#!/usr/bin/env -S uv run --script
"""Format HEAD's commit message and amend only when formatting changes it."""

from __future__ import annotations

import re
import subprocess
import sys
import textwrap

# Keep commit messages at 60 columns so they stay readable over phone SSH.
WIDTH = 60
TRAILER = re.compile(r"^[A-Za-z][A-Za-z0-9-]*:")


def wrap_paragraph(lines: list[str]) -> list[str]:
    text = " ".join(line.strip() for line in lines)
    return textwrap.wrap(text, width=WIDTH) or [""]


def format_message(message: str) -> str:
    lines = message.rstrip("\n").splitlines()
    if not lines:
        return message

    result = [lines[0], ""]
    paragraph: list[str] = []

    def flush() -> None:
        if paragraph:
            result.extend(wrap_paragraph(paragraph))
            paragraph.clear()

    index = 1
    while index < len(lines):
        line = lines[index]
        if not line.strip():
            flush()
            if result[-1] != "":
                result.append("")
            index += 1
            continue
        if line.startswith(("- ", "* ")):
            flush()
            bullet_lines = [line[2:].strip()]
            index += 1
            while index < len(lines) and lines[index].startswith(("  ", "\t")):
                bullet_lines.append(lines[index].strip())
                index += 1
            wrapped = textwrap.wrap(
                " ".join(bullet_lines),
                width=WIDTH,
                initial_indent=line[:2],
                subsequent_indent="  ",
            )
            result.extend(wrapped or [line[:2]])
            continue
        if TRAILER.match(line) or line.endswith(":"):
            flush()
            result.append(line)
            index += 1
            continue
        paragraph.append(line)
        index += 1

    flush()
    while result and result[-1] == "":
        result.pop()
    return "\n".join(result) + "\n"


def main() -> int:
    commit = subprocess.run(
        ["git", "cat-file", "commit", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    _, original = commit.split("\n\n", 1)
    formatted = format_message(original)
    if formatted == original:
        print("commit message already formatted")
        return 0

    print("amending commit with formatted message")
    subprocess.run(
        ["git", "commit", "--amend", "-F", "-"], input=formatted, text=True, check=True
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
