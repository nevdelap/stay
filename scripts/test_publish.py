#!/usr/bin/env -S uv run --script
"""Exercise the operator-only publish recipe without external side effects."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def git(repo: Path, *arguments: str) -> None:
    subprocess.run(
        ["git", *arguments], cwd=repo, check=True, capture_output=True, text=True
    )


class PublishRecipeTests(unittest.TestCase):
    """Run each recipe case in a clean, disposable Git fixture."""

    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        temporary_root = Path(self.temp_dir.name)
        self.repo = temporary_root / "repo"
        self.mock_bin = temporary_root / "mock-bin"
        self.log = temporary_root / "commands.log"
        self.repo.mkdir()
        self.mock_bin.mkdir()
        (self.repo / "justfile").write_text(
            (ROOT / "justfile").read_text(), encoding="utf-8"
        )
        (self.repo / "fixture.txt").write_text("fixture\n", encoding="utf-8")
        git(self.repo, "init", "--quiet")
        git(self.repo, "config", "user.name", "Publish Recipe Tests")
        git(self.repo, "config", "user.email", "publish@example.test")
        git(self.repo, "add", "justfile", "fixture.txt")
        git(self.repo, "commit", "--quiet", "-m", "fixture")
        self._write_mock(
            "cargo",
            """#!/usr/bin/env python3
import os
import sys

log = os.environ["MOCK_LOG"]
args = sys.argv[1:]
with open(log, "a", encoding="utf-8") as output:
    output.write("cargo " + " ".join(args) + "\\n")
if args and args[0] == "metadata":
    metadata = os.environ.get("MOCK_METADATA")
    if metadata == "invalid":
        print("{")
    elif metadata == "non-single":
        print('{"packages":[{"name":"stay","version":"0.0.49"},{"name":"other","version":"0.0.1"}]}')
    else:
        print('{"packages":[{"name":"stay","version":"0.0.49"}]}')
    raise SystemExit(0)
if args == ["publish", "--locked", "--dry-run"]:
    if os.environ.get("MOCK_DRY_RUN") == "fail":
        raise SystemExit(23)
    raise SystemExit(0)
if args == ["publish", "--locked"]:
    raise SystemExit(0)
raise SystemExit(99)
""",
        )
        self._write_mock(
            "jq",
            """#!/usr/bin/env python3
import json
import os
import sys

with open(os.environ["MOCK_LOG"], "a", encoding="utf-8") as output:
    output.write("jq\\n")
try:
    metadata = json.load(sys.stdin)
    packages = metadata["packages"]
    package = packages[0]
    if len(packages) != 1 or package["name"] != "stay":
        raise ValueError
except (KeyError, IndexError, TypeError, ValueError, json.JSONDecodeError):
    raise SystemExit(1)
print(package["version"])
""",
        )
        self._write_mock(
            "curl",
            """#!/usr/bin/env python3
import os
import sys

with open(os.environ["MOCK_LOG"], "a", encoding="utf-8") as output:
    output.write("curl " + " ".join(sys.argv[1:]) + "\\n")
if os.environ.get("MOCK_CURL") == "network":
    raise SystemExit(7)
print(os.environ.get("MOCK_STATUS", "404"), end="")
""",
        )

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def _write_mock(self, name: str, contents: str) -> None:
        path = self.mock_bin / name
        path.write_text(contents, encoding="utf-8")
        path.chmod(0o755)

    def run_publish(self, **updates: str) -> subprocess.CompletedProcess[str]:
        just = shutil.which("just")
        if just is None:
            self.fail("just is required to test the publish recipe")
        self.log.unlink(missing_ok=True)
        environment = os.environ.copy()
        # The fixture must reach the recipe's mocked safety checks in CI. The
        # dedicated refusal test below restores each marker explicitly.
        environment.pop("CI", None)
        environment.pop("GITHUB_ACTIONS", None)
        environment.update(
            {
                "PATH": f"{self.mock_bin}{os.pathsep}{environment['PATH']}",
                "MOCK_LOG": str(self.log),
            }
        )
        environment.update(updates)
        return subprocess.run(
            [just, "--justfile", str(self.repo / "justfile"), "publish"],
            cwd=self.repo,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

    def commands(self) -> list[str]:
        if not self.log.exists():
            return []
        return self.log.read_text(encoding="utf-8").splitlines()

    def assert_refused_before_commands(
        self, result: subprocess.CompletedProcess[str]
    ) -> None:
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.commands(), [])

    def test_refuses_ci_execution_for_both_ci_markers(self) -> None:
        for marker in ("CI", "GITHUB_ACTIONS"):
            with self.subTest(marker=marker):
                self.assert_refused_before_commands(
                    self.run_publish(**{marker: "true"})
                )

    def test_refuses_dirty_worktree(self) -> None:
        (self.repo / "untracked.txt").write_text("dirty\n", encoding="utf-8")

        self.assert_refused_before_commands(self.run_publish())

    def test_refuses_invalid_and_non_single_package_metadata(self) -> None:
        for metadata in ("invalid", "non-single"):
            with self.subTest(metadata=metadata):
                result = self.run_publish(MOCK_METADATA=metadata)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("jq", self.commands())
                self.assertIn(
                    "cargo metadata --format-version 1 --no-deps", self.commands()
                )
                self.assertNotIn("cargo publish --locked --dry-run", self.commands())
                self.assertNotIn("cargo publish --locked", self.commands())

    def test_stops_when_dry_run_fails(self) -> None:
        result = self.run_publish(MOCK_DRY_RUN="fail")

        self.assertNotEqual(result.returncode, 0)
        commands = self.commands()
        self.assertIn("cargo metadata --format-version 1 --no-deps", commands)
        self.assertIn("jq", commands)
        self.assertIn("cargo publish --locked --dry-run", commands)
        self.assertNotIn("cargo publish --locked", commands)
        self.assertNotIn("curl", " ".join(commands))

    def test_stops_when_registry_query_fails(self) -> None:
        result = self.run_publish(MOCK_CURL="network")

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.commands()[-1].split()[0], "curl")
        self.assertNotIn("cargo publish --locked", self.commands())

    def test_refuses_every_representative_non_404_response(self) -> None:
        for status in ("100", "200", "301", "400", "500", "599"):
            with self.subTest(status=status):
                result = self.run_publish(MOCK_STATUS=status)

                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("cargo publish --locked", self.commands())

    def test_orders_dry_run_ownership_check_and_single_real_publish(self) -> None:
        result = self.run_publish()

        self.assertEqual(result.returncode, 0, result.stderr)
        commands = self.commands()
        dry_run = "cargo publish --locked --dry-run"
        real_publish = "cargo publish --locked"
        curl = next(
            index
            for index, command in enumerate(commands)
            if command.startswith("curl ")
        )
        self.assertIn(
            "--header User-Agent: stay-release-bootstrap/0.1 "
            "(https://github.com/nevdelap/stay)",
            commands[curl],
        )
        self.assertIn("cargo metadata --format-version 1 --no-deps", commands)
        self.assertIn("jq", commands)
        self.assertLess(
            commands.index("cargo metadata --format-version 1 --no-deps"),
            commands.index(dry_run),
        )
        self.assertLess(commands.index("jq"), commands.index(dry_run))
        self.assertLess(commands.index(dry_run), curl)
        self.assertLess(curl, commands.index(real_publish))
        self.assertEqual(commands.count(real_publish), 1)


if __name__ == "__main__":
    unittest.main()
