#!/usr/bin/env -S uv run --script
"""Run repository quality checks against a Git-selected file set.

File checks honor the requested scope; commit-message formatting and linting
remain explicit whole-commit operations because they are not file-granular.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
from collections.abc import Iterable, Sequence
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MD_IMAGE = "stay-mdformat:latest"
CACHE_ROOT = Path(tempfile.gettempdir())
BUILDX_CONFIG = str(CACHE_ROOT / "stay-buildx")
UV_CACHE_DIR = str(CACHE_ROOT / "stay-uv-cache")
UV_TOOL_DIR = str(CACHE_ROOT / "stay-uv-tools")
COMMIT_MESSAGE_WIDTH = 60
COMMIT_TRAILER = re.compile(r"^[A-Za-z][A-Za-z0-9-]*:")
DEBUGGING_MACROS = (
    "dbg!",
    "todo!",
    "unimplemented!",
    "print!",
    "println!",
    "eprint!",
    "eprintln!",
)
INTENTIONAL_OUTPUT_MARKER = "// quality: intentional-output"


def run(
    command: Sequence[str], *, capture: bool = False
) -> subprocess.CompletedProcess[bytes]:
    """Run one quality command from the repository root."""

    return subprocess.run(
        list(command),
        cwd=ROOT,
        check=True,
        capture_output=capture,
    )


def git(*arguments: str) -> bytes:
    return run(["git", *arguments], capture=True).stdout


def parse_name_status_z(output: bytes) -> list[str]:
    """Return existing destination paths from `git diff --name-status -z`."""

    fields = output.split(b"\0")
    paths: list[str] = []
    index = 0
    while index < len(fields) and fields[index]:
        status = os.fsdecode(fields[index])
        index += 1
        if status[:1] in {"C", "R"}:
            if index + 1 >= len(fields):
                raise ValueError(f"malformed rename/copy status: {status!r}")
            index += 1
            paths.append(os.fsdecode(fields[index]))
            index += 1
        else:
            if index >= len(fields):
                raise ValueError(f"malformed status: {status!r}")
            paths.append(os.fsdecode(fields[index]))
            index += 1
    return paths


def _staged_changes_exist() -> bool:
    result = subprocess.run(
        ["git", "diff", "--cached", "--quiet", "HEAD", "--"],
        cwd=ROOT,
        check=False,
    )
    if result.returncode not in {0, 1}:
        raise RuntimeError("could not inspect staged changes")
    return result.returncode == 1


def _commit_paths() -> list[str]:
    try:
        git("rev-parse", "--verify", "HEAD^")
    except subprocess.CalledProcessError:
        return parse_name_status_z(
            git(
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-status",
                "-z",
                "--find-copies-harder",
                "-r",
                "HEAD",
                "--",
            )
        )
    return parse_name_status_z(
        git(
            "diff",
            "--name-status",
            "-z",
            "--find-copies-harder",
            "HEAD^",
            "HEAD",
            "--",
        )
    )


def selected_paths(scope: str) -> list[str]:
    """Select existing, tracked, non-generated paths for one quality run."""

    if scope == "all":
        candidates = os.fsdecode(git("ls-files", "-z")).split("\0")[:-1]
    elif scope == "changed":
        if _staged_changes_exist():
            candidates = parse_name_status_z(
                git(
                    "diff",
                    "--cached",
                    "--name-status",
                    "-z",
                    "--find-copies-harder",
                    "HEAD",
                    "--",
                )
            )
        else:
            candidates = _commit_paths()
    else:
        raise ValueError(f"unknown quality scope: {scope!r}")

    result: list[str] = []
    seen: set[str] = set()
    for candidate in candidates:
        path = ROOT / candidate
        if (
            candidate in seen
            or candidate.startswith((".git/", "target/", "target-mac/"))
            or candidate == "check.log"
            or path.is_symlink()
            or not path.is_file()
        ):
            continue
        seen.add(candidate)
        result.append(candidate)
    return result


def classify(paths: Iterable[str]) -> dict[str, list[str]]:
    """Classify paths once so format and lint use the same matrix."""

    result = {
        "bash": [],
        "docker": [],
        "json": [],
        "just": [],
        "markdown": [],
        "python": [],
        "rust": [],
        "toml": [],
        "yaml": [],
    }
    for path in paths:
        suffix = Path(path).suffix.lower()
        name = Path(path).name
        if suffix == ".py":
            result["python"].append(path)
        elif (suffix == ".sh" or path.startswith("scripts/")) and suffix != ".py":
            result["bash"].append(path)
        elif name.startswith("Dockerfile"):
            result["docker"].append(path)
        elif suffix == ".json":
            result["json"].append(path)
        elif path == "justfile":
            result["just"].append(path)
        elif suffix == ".md":
            result["markdown"].append(path)
        elif suffix == ".rs":
            result["rust"].append(path)
        elif suffix == ".toml":
            result["toml"].append(path)
        elif suffix in {".yaml", ".yml"}:
            result["yaml"].append(path)
    return result


def _docker(
    image: str, arguments: Sequence[str], *, user: bool = False, pull: bool = False
) -> list[str]:
    command = ["docker", "run"]
    if pull:
        command.extend(["--pull", "always"])
    command.extend(["--rm"])
    if user:
        command.extend(["-u", f"{os.getuid()}:{os.getgid()}"])
    command.extend(["-v", f"{ROOT}:/workdir", "-w", "/workdir", image])
    command.extend(arguments)
    return command


def _workdir_paths(paths: Iterable[str]) -> list[str]:
    return [f"/workdir/{path}" for path in paths]


def _uv_environment() -> dict[str, str]:
    environment = os.environ.copy()
    environment.update({"UV_CACHE_DIR": UV_CACHE_DIR, "UV_TOOL_DIR": UV_TOOL_DIR})
    return environment


def _run_uv(arguments: Sequence[str]) -> None:
    subprocess.run(
        ["uv", "tool", "run", *arguments], cwd=ROOT, env=_uv_environment(), check=True
    )


def _format_json(paths: Sequence[str]) -> None:
    for path in paths:
        with tempfile.NamedTemporaryFile(dir=ROOT, delete=False) as temporary:
            temporary_path = Path(temporary.name)
        try:
            output = subprocess.run(
                _docker(
                    "ghcr.io/jqlang/jq:latest",
                    ["--sort-keys", ".", f"/workdir/{path}"],
                ),
                cwd=ROOT,
                check=True,
                capture_output=True,
            ).stdout
            temporary_path.write_bytes(output)
            os.replace(temporary_path, ROOT / path)
        finally:
            temporary_path.unlink(missing_ok=True)


def _lint_json(paths: Sequence[str]) -> None:
    if paths:
        run(_docker("ghcr.io/jqlang/jq:latest", ["empty", *_workdir_paths(paths)]))


def _format_markdown(paths: Sequence[str]) -> None:
    paths = [path for path in paths if not path.startswith("review_docs/")]
    if not paths:
        return
    environment = os.environ.copy()
    environment["BUILDX_CONFIG"] = BUILDX_CONFIG
    subprocess.run(
        ["docker", "build", "-q", "-t", MD_IMAGE, "docker/mdformat"],
        cwd=ROOT,
        env=environment,
        check=True,
        capture_output=True,
    )
    run(_docker(MD_IMAGE, _workdir_paths(paths), user=True))


def _lint_markdown(paths: Sequence[str]) -> None:
    paths = [path for path in paths if not path.startswith("review_docs/")]
    if paths:
        run(
            _docker(
                "ghcr.io/igorshubovych/markdownlint-cli:latest",
                _workdir_paths(paths),
                user=True,
            )
        )


def _format_just(paths: Sequence[str]) -> None:
    if paths:
        run(["just", "--fmt", "--unstable"])


def _format_python(paths: Sequence[str]) -> None:
    if not paths:
        return
    _run_uv(
        [
            "--from",
            "pyupgrade",
            "pyupgrade",
            "--py39-plus",
            "--exit-zero-even-if-changed",
            *paths,
        ]
    )
    _run_uv(["ruff", "check", "--fix", *paths])
    _run_uv(["ruff", "format", *paths])


def _lint_python(paths: Sequence[str]) -> None:
    if not paths:
        return
    _run_uv(["ruff", "check", *paths])
    _run_uv(["ty", "check", *paths])
    _run_uv(["bandit", "-q", "-r", *paths, "-c", ".bandit.yml"])


def _format_rust(paths: Sequence[str], all_files: bool) -> None:
    if all_files:
        run(["cargo", "fmt", "--all"])
    else:
        for path in paths:
            run(["rustfmt", "--edition", "2024", "--config-path", "rustfmt.toml", path])


def _same_source_path(file_name: str, changed: set[str]) -> bool:
    source = Path(file_name)
    if source.is_absolute():
        try:
            source = source.resolve().relative_to(ROOT.resolve())
        except ValueError:
            return False
    return source.as_posix() in changed


def rust_diagnostics(output: bytes) -> list[dict]:
    """Return warning and error compiler diagnostics from Cargo output."""

    diagnostics: list[dict] = []
    for line in output.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        diagnostic = message.get("message", {})
        if diagnostic.get("level") not in {"warning", "error"}:
            continue
        diagnostics.append(diagnostic)
    return diagnostics


def changed_rust_diagnostics(output: bytes, paths: Sequence[str]) -> list[str]:
    """Return compiler diagnostics whose source spans touch changed files."""

    changed = {Path(path).as_posix() for path in paths}
    relevant: list[str] = []
    for diagnostic in rust_diagnostics(output):
        spans = diagnostic.get("spans", [])
        if any(_same_source_path(span.get("file_name", ""), changed) for span in spans):
            relevant.append(diagnostic.get("rendered", json.dumps(diagnostic)))
    return relevant


def _lint_rust(paths: Sequence[str], all_files: bool) -> None:
    if all_files:
        run(
            [
                "cargo",
                "clippy",
                "--locked",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ]
        )
        return
    if not paths:
        return
    # Cargo may reuse a warm package fingerprint without emitting compiler
    # diagnostics. Clean only this package so the changed-file gate always
    # analyzes the selected source while preserving dependency artifacts.
    run(["cargo", "clean", "--package", "stay"])
    result = subprocess.run(
        [
            "cargo",
            "clippy",
            "--locked",
            "--all-targets",
            "--all-features",
            "--message-format=json",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
    )
    diagnostics = rust_diagnostics(result.stdout)
    relevant = changed_rust_diagnostics(result.stdout, paths)
    if relevant:
        print("changed Rust files have Clippy diagnostics:", file=sys.stderr)
        print("".join(relevant), file=sys.stderr, end="")
        raise RuntimeError("changed Rust files failed Clippy")
    if result.returncode and not diagnostics:
        sys.stderr.buffer.write(result.stderr)
        raise subprocess.CalledProcessError(result.returncode, result.args)


def _format_toml(paths: Sequence[str]) -> None:
    if paths:
        run(
            _docker(
                "tamasfe/taplo:latest",
                ["format", *_workdir_paths(paths)],
                user=True,
                pull=True,
            )
        )


def _lint_toml(paths: Sequence[str]) -> None:
    if paths:
        run(
            _docker(
                "tamasfe/taplo:latest",
                ["check", *_workdir_paths(paths)],
                user=True,
                pull=True,
            )
        )


def _format_yaml(paths: Sequence[str]) -> None:
    if paths:
        run(
            _docker(
                "ghcr.io/google/yamlfmt:latest",
                _workdir_paths(paths),
                user=True,
                pull=True,
            )
        )


def _lint_yaml(paths: Sequence[str]) -> None:
    if paths:
        run(
            _docker(
                "ghcr.io/ffurrer2/yamllint:latest",
                _workdir_paths(paths),
                user=True,
                pull=True,
            )
        )


def _format_bash(paths: Sequence[str]) -> None:
    if paths:
        run(
            _docker(
                "mvdan/shfmt:v3",
                ["-w", "-i", "4", "-ci", *_workdir_paths(paths)],
                pull=True,
            )
        )


def _lint_bash(paths: Sequence[str]) -> None:
    if paths:
        run(
            _docker(
                "koalaman/shellcheck:stable",
                ["--external-sources", *_workdir_paths(paths)],
                pull=True,
            )
        )


def _format_docker(paths: Sequence[str]) -> None:
    if paths:
        _run_uv(
            [
                "--from",
                "tally-cli",
                "tally",
                "lint",
                "--fix",
                "--fail-level",
                "none",
                "--slow-checks",
                "off",
                "--ignore",
                "hadolint/DL3007",
                "--ignore",
                "tally/prefer-package-cache-mounts",
                *paths,
            ]
        )


def _lint_docker(paths: Sequence[str]) -> None:
    for path in paths:
        with (ROOT / path).open("rb") as source:
            subprocess.run(
                _docker(
                    "hadolint/hadolint:latest",
                    ["/bin/hadolint", "--ignore", "DL3007", "-"],
                    pull=True,
                ),
                cwd=ROOT,
                stdin=source,
                check=True,
            )


def _lint_actionlint(paths: Sequence[str]) -> None:
    workflow_paths = [path for path in paths if path.startswith(".github/workflows/")]
    if workflow_paths:
        run(
            _docker(
                "rhysd/actionlint:latest", _workdir_paths(workflow_paths), pull=True
            )
        )


def _lint_no_debugging(paths: Sequence[str], all_files: bool) -> None:
    selected = [path for path in paths if path.startswith(("src/", "tests/"))]
    if not selected and not all_files:
        return
    pattern = "|".join(re.escape(macro) for macro in DEBUGGING_MACROS)
    command = ["rg", "-n", "--with-filename", pattern]
    command.extend(["src", "tests"] if all_files else selected)
    result = subprocess.run(
        command, cwd=ROOT, check=False, capture_output=True, text=True
    )
    if result.returncode == 0:
        violations: list[str] = []
        for match in result.stdout.splitlines():
            path_name, line_number, _ = match.split(":", 2)
            path = ROOT / path_name
            lines = path.read_text().splitlines()
            line_index = int(line_number) - 1
            if (
                line_index == 0
                or lines[line_index - 1].strip() != INTENTIONAL_OUTPUT_MARKER
            ):
                violations.append(match)
        if violations:
            print("stray debugging macro found:", file=sys.stderr)
            print("\n".join(violations), file=sys.stderr)
            raise RuntimeError("stray debugging macro found")
        return
    if result.returncode != 1:
        raise subprocess.CalledProcessError(result.returncode, command)


def _gitlint_target_args() -> list[str]:
    """Lint the PR head when Actions checks out a synthetic merge commit."""

    if os.environ.get("GITHUB_ACTIONS") != "true":
        return []

    result = subprocess.run(
        ["git", "rev-parse", "--verify", "HEAD^2"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return []

    merge_parent = result.stdout.strip()
    return ["--commit", merge_parent] if merge_parent else []


def _lint_commit() -> None:
    command = [
        "docker",
        "run",
        "--pull",
        "always",
        "--rm",
        "-v",
        f"{ROOT}:/repo",
        "-w",
        "/repo",
        "jorisroovers/gitlint:latest",
        "--config",
        ".gitlint",
    ]
    command.extend(_gitlint_target_args())
    run(command)


def _wrap_commit_paragraph(lines: Sequence[str]) -> list[str]:
    text = " ".join(line.strip() for line in lines)
    return textwrap.wrap(text, width=COMMIT_MESSAGE_WIDTH) or [""]


def format_commit_message(message: str) -> str:
    """Format a commit message without changing its meaning."""

    lines = message.rstrip("\n").splitlines()
    if not lines:
        return message

    result = [lines[0], ""]
    paragraph: list[str] = []

    def flush() -> None:
        if paragraph:
            result.extend(_wrap_commit_paragraph(paragraph))
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
                width=COMMIT_MESSAGE_WIDTH,
                initial_indent=line[:2],
                subsequent_indent="  ",
            )
            result.extend(wrapped or [line[:2]])
            continue
        if COMMIT_TRAILER.match(line) or line.endswith(":"):
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


def format_current_commit_message() -> int:
    """Amend HEAD only when its commit message needs formatting."""

    commit = subprocess.run(
        ["git", "cat-file", "commit", "HEAD"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    _, separator, original = commit.partition("\n\n")
    if not separator:
        raise RuntimeError("HEAD has no commit message separator")
    formatted = format_commit_message(original)
    if formatted == original:
        print("commit message already formatted")
        return 0

    print("amending commit with formatted message")
    subprocess.run(
        ["git", "commit", "--amend", "--only", "-F", "-"],
        cwd=ROOT,
        input=formatted,
        text=True,
        check=True,
    )
    return 0


def format_files(paths: Sequence[str], all_files: bool) -> None:
    groups = classify(paths)
    _format_bash(groups["bash"])
    _format_docker(groups["docker"])
    _format_json(groups["json"])
    _format_just(groups["just"])
    _format_markdown(groups["markdown"])
    _format_python(groups["python"])
    _format_rust(groups["rust"], all_files)
    _format_toml(groups["toml"])
    _format_yaml(groups["yaml"])


def lint_files(paths: Sequence[str], all_files: bool) -> None:
    groups = classify(paths)
    _lint_actionlint(groups["yaml"])
    _lint_bash(groups["bash"])
    _lint_commit()
    _lint_docker(groups["docker"])
    _lint_json(groups["json"])
    _lint_markdown(groups["markdown"])
    _lint_no_debugging(paths, all_files)
    _lint_python(groups["python"])
    _lint_rust(groups["rust"], all_files)
    _lint_toml(groups["toml"])
    _lint_yaml(groups["yaml"])


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="operation", required=True)
    for operation in ("format", "lint"):
        operation_parser = subparsers.add_parser(operation)
        operation_parser.add_argument(
            "--scope", choices=["changed", "all"], default="changed"
        )
    subparsers.add_parser("commit-message")
    arguments = parser.parse_args(argv)
    if arguments.operation == "commit-message":
        return format_current_commit_message()

    paths = selected_paths(arguments.scope)
    all_files = arguments.scope == "all"
    if arguments.operation == "format":
        format_files(paths, all_files)
    else:
        lint_files(paths, all_files)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
