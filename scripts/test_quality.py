#!/usr/bin/env -S uv run --script
"""Tests for the repository quality dispatcher."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

try:
    from scripts import quality
except ModuleNotFoundError:
    import quality


def git(repo: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *arguments],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )


class QualityDispatcherTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.repo = Path(self.temporary_directory.name)
        git(self.repo, "init", "--quiet")
        git(self.repo, "config", "user.name", "Quality Tests")
        git(self.repo, "config", "user.email", "quality@example.test")
        files = {
            ".github/workflows/ci.yml": "name: ci\n",
            "Dockerfile": "FROM scratch\n",
            "config.toml": "[package]\nname = 'fixture'\n",
            "config.yaml": "name: fixture\n",
            "changed.md": "# Changed\n",
            "data.json": '{"name":"fixture"}\n',
            "justfile": "default:\n    @true\n",
            "notes.md": "# Notes\n",
            "script.sh": "#!/bin/sh\necho fixture\n",
            "scripts/tool.py": "print('fixture')\n",
            "src/changed.rs": "fn main() {}\n",
            "src/unchanged.rs": "fn unchanged() {}\n",
            "tests/example.rs": "#[test]\nfn example() {}\n",
            "unchanged.md": "invalid fixture\n",
        }
        for name, contents in files.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents)
        git(self.repo, "add", ".")
        git(self.repo, "commit", "--quiet", "-m", "initial")

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def selected(self, scope: str) -> list[str]:
        with patch.object(quality, "ROOT", self.repo):
            return quality.selected_paths(scope)

    def test_staged_selection_uses_rename_destination_and_ignores_deletion(
        self,
    ) -> None:
        (self.repo / "notes.md").unlink()
        git(self.repo, "mv", "src/changed.rs", "src/renamed.rs")
        (self.repo / "src/copied.rs").write_text(
            (self.repo / "src/unchanged.rs").read_text()
        )
        paths = self.selected_and_stage_all()

        self.assertIn("src/renamed.rs", paths)
        self.assertIn("src/copied.rs", paths)
        self.assertNotIn("src/changed.rs", paths)
        self.assertNotIn("notes.md", paths)

    def selected_and_stage_all(self) -> list[str]:
        git(self.repo, "add", "-A")
        return self.selected("changed")

    def test_clean_selection_uses_current_commit_parent_diff(self) -> None:
        (self.repo / "notes.md").write_text("# Changed\n")
        git(self.repo, "add", "notes.md")
        git(self.repo, "commit", "--quiet", "-m", "change notes")

        self.assertEqual(self.selected("changed"), ["notes.md"])

    def test_all_selection_includes_unchanged_tracked_files(self) -> None:
        paths = set(self.selected("all"))

        self.assertIn("src/unchanged.rs", paths)
        self.assertIn("notes.md", paths)
        self.assertNotIn("check.log", paths)

    def test_classification_covers_the_tool_matrix(self) -> None:
        groups = quality.classify(
            [
                ".github/workflows/ci.yml",
                "Dockerfile",
                "config.toml",
                "config.yaml",
                "data.json",
                "justfile",
                "notes.md",
                "script.sh",
                "scripts/tool.py",
                "src/changed.rs",
            ]
        )

        self.assertEqual(groups["bash"], ["script.sh"])
        self.assertEqual(groups["docker"], ["Dockerfile"])
        self.assertEqual(groups["json"], ["data.json"])
        self.assertEqual(groups["just"], ["justfile"])
        self.assertEqual(groups["markdown"], ["notes.md"])
        self.assertEqual(groups["python"], ["scripts/tool.py"])
        self.assertEqual(groups["rust"], ["src/changed.rs"])
        self.assertEqual(groups["toml"], ["config.toml"])
        self.assertEqual(groups["yaml"], [".github/workflows/ci.yml", "config.yaml"])

    def test_empty_file_selection_is_a_noop_for_file_tools(self) -> None:
        formatters = {
            name: Mock()
            for name in (
                "_format_bash",
                "_format_docker",
                "_format_json",
                "_format_just",
                "_format_markdown",
                "_format_python",
                "_format_rust",
                "_format_toml",
                "_format_yaml",
            )
        }
        with patch.multiple(quality, **formatters):
            quality.format_files([], all_files=False)

        formatters["_format_rust"].assert_called_once_with([], False)
        for name, formatter in formatters.items():
            if name != "_format_rust":
                formatter.assert_called_once_with([])

    def test_dispatch_receives_only_selected_files(self) -> None:
        formatter = Mock()
        with patch.object(quality, "_format_markdown", formatter):
            quality.format_files(["notes.md"], all_files=False)

        formatter.assert_called_once_with(["notes.md"])

    def test_fixture_ignores_unchanged_format_violation_in_changed_scope(self) -> None:
        (self.repo / "changed.md").write_text("# Changed\n\nupdated\n")
        git(self.repo, "add", "changed.md")

        def formatter(paths: list[str]) -> None:
            violations = [
                path
                for path in paths
                if (self.repo / path).read_text().startswith("invalid")
            ]
            if violations:
                raise AssertionError("unchanged formatting violation was found")

        formatters = {
            name: Mock()
            for name in (
                "_format_bash",
                "_format_docker",
                "_format_json",
                "_format_just",
                "_format_python",
                "_format_rust",
                "_format_toml",
                "_format_yaml",
            )
        }
        formatters["_format_markdown"] = Mock(side_effect=formatter)
        with (
            patch.object(quality, "ROOT", self.repo),
            patch.multiple(quality, **formatters),
        ):
            quality.main(["format", "--scope", "changed"])
            with self.assertRaises(AssertionError):
                quality.main(["format", "--scope", "all"])

    def test_fixture_ignores_unchanged_lint_violation_in_changed_scope(self) -> None:
        (self.repo / "changed.md").write_text("# Changed\n\nupdated\n")
        git(self.repo, "add", "changed.md")

        def linter(paths: list[str]) -> None:
            violations = [
                path
                for path in paths
                if (self.repo / path).read_text().startswith("invalid")
            ]
            if violations:
                raise AssertionError("unchanged lint violation was found")

        linters = {
            name: Mock()
            for name in (
                "_lint_actionlint",
                "_lint_bash",
                "_lint_commit",
                "_lint_docker",
                "_lint_json",
                "_lint_no_debugging",
                "_lint_python",
                "_lint_rust",
                "_lint_toml",
                "_lint_yaml",
            )
        }
        linters["_lint_markdown"] = Mock(side_effect=linter)
        with (
            patch.object(quality, "ROOT", self.repo),
            patch.multiple(quality, **linters),
        ):
            quality.main(["lint", "--scope", "changed"])
            with self.assertRaises(AssertionError):
                quality.main(["lint", "--scope", "all"])

    def test_empty_lint_selection_is_a_noop_for_file_tools(self) -> None:
        linters = {
            name: Mock()
            for name in (
                "_lint_actionlint",
                "_lint_bash",
                "_lint_commit",
                "_lint_docker",
                "_lint_json",
                "_lint_markdown",
                "_lint_no_debugging",
                "_lint_python",
                "_lint_rust",
                "_lint_toml",
                "_lint_yaml",
            )
        }
        with patch.multiple(quality, **linters):
            quality.lint_files([], all_files=False)

        linters["_lint_commit"].assert_called_once_with()
        for name, linter in linters.items():
            if name == "_lint_commit":
                continue
            linter.assert_called_once()

    def test_json_lint_batches_files_into_one_container(self) -> None:
        with (
            patch.object(quality, "ROOT", Path("/repo")),
            patch.object(quality, "run") as run,
        ):
            quality._lint_json(["one.json", "two.json"])

        run.assert_called_once()
        command = run.call_args.args[0]
        self.assertEqual(command.count("ghcr.io/jqlang/jq:latest"), 1)
        self.assertIn("/workdir/one.json", command)
        self.assertIn("/workdir/two.json", command)

    def test_commit_message_formatting_amends_only_when_needed(self) -> None:
        (self.repo / "notes.md").write_text("# Commit message fixture\n")
        git(self.repo, "add", "notes.md")
        git(self.repo, "commit", "--quiet", "-m", "summary", "-m", "a long body " * 8)
        (self.repo / "staged.txt").write_text("staged\n")
        git(self.repo, "add", "staged.txt")
        with patch.object(quality, "ROOT", self.repo):
            quality.format_current_commit_message()
        message = git(self.repo, "show", "-s", "--format=%B", "HEAD").stdout

        self.assertLessEqual(max(map(len, message.splitlines())), 60)
        self.assertIn("summary\n\n", message)
        self.assertIn(
            "staged.txt", git(self.repo, "diff", "--cached", "--name-only").stdout
        )

    def test_commit_lint_targets_pr_head_on_github_merge_checkout(self) -> None:
        git_result = subprocess.CompletedProcess(
            args=["git"], returncode=0, stdout="pr-head\n", stderr=""
        )
        with (
            patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}),
            patch.object(quality, "ROOT", self.repo),
            patch.object(quality.subprocess, "run", return_value=git_result),
            patch.object(quality, "run") as run,
        ):
            quality._lint_commit()

        command = run.call_args.args[0]
        self.assertEqual(command[-2:], ["--commit", "pr-head"])

    def test_commit_lint_keeps_default_target_outside_github_merge_checkout(
        self,
    ) -> None:
        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(quality, "ROOT", self.repo),
            patch.object(quality, "run") as run,
        ):
            quality._lint_commit()

        self.assertNotIn("--commits", run.call_args.args[0])

    def test_rust_diagnostic_filtering_uses_changed_source_spans(self) -> None:
        output = b"\n".join(
            json.dumps(
                {
                    "reason": "compiler-message",
                    "message": {
                        "level": level,
                        "rendered": rendered,
                        "spans": [{"file_name": file_name}],
                    },
                }
            ).encode()
            for rendered, file_name, level in (
                ("changed\n", "src/changed.rs", "warning"),
                ("unchanged\n", "src/unchanged.rs", "warning"),
                ("unchanged error\n", "src/unchanged.rs", "error"),
            )
        )

        relevant = quality.changed_rust_diagnostics(output, ["src/changed.rs"])

        self.assertEqual(relevant, ["changed\n"])

    def test_unchanged_clippy_error_does_not_fail_changed_lint(self) -> None:
        output = json.dumps(
            {
                "reason": "compiler-message",
                "message": {
                    "level": "error",
                    "rendered": "unchanged error\n",
                    "spans": [{"file_name": "src/unchanged.rs"}],
                },
            }
        ).encode()
        result = subprocess.CompletedProcess(
            ["cargo", "clippy"], 101, stdout=output, stderr=b""
        )

        with patch.object(quality.subprocess, "run", return_value=result):
            quality._lint_rust(["src/changed.rs"], all_files=False)


if __name__ == "__main__":
    unittest.main()
