"""Tests for selecting only the CLI executable reported by Cargo."""

import json
from pathlib import Path
import tempfile
import unittest

from cargo_artifact_executable import select_executable


class CargoArtifactExecutableTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.manifest = self.root / "crates/velnor-actions-cli/Cargo.toml"
        self.manifest.parent.mkdir(parents=True)
        self.manifest.write_text("[package]\nname = 'velnor-actions-cli'\n")

    def executable(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("fixture executable\n")
        path.chmod(0o700)
        return path

    def artifact(self, executable, manifest=None, target="velnor-actions", kind=None):
        return json.dumps({
            "reason": "compiler-artifact",
            "manifest_path": str(manifest or self.manifest),
            "target": {"name": target, "kind": kind or ["bin"]},
            "executable": str(executable) if executable is not None else None,
        })

    def select(self, messages):
        return select_executable(
            messages, self.root, self.manifest.relative_to(self.root), "velnor-actions"
        )

    def test_external_build_artifact_wins_over_stale_repository_binary(self):
        stale = self.executable(self.root / "target/debug/velnor-actions")
        current = self.executable(self.root / "private-target/debug/velnor-actions")
        unrelated = self.artifact(stale, target="dependency-tool")
        self.assertEqual(self.select([unrelated, self.artifact(current)]), current.resolve())

    def test_relative_target_artifact_resolves_from_workspace_root(self):
        relative = Path("custom-target/debug/velnor-actions")
        expected = self.executable(self.root / relative)
        self.assertEqual(self.select([self.artifact(relative)]), expected.resolve())

    def test_missing_executable_fails_closed(self):
        missing = self.root / "external-target/debug/velnor-actions"
        with self.assertRaisesRegex(ValueError, "does not exist"):
            self.select([self.artifact(missing)])

    def test_missing_or_ambiguous_artifact_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "found 0"):
            self.select([json.dumps({"reason": "build-finished", "success": True})])
        first = self.executable(self.root / "target-a/velnor-actions")
        second = self.executable(self.root / "target-b/velnor-actions")
        with self.assertRaisesRegex(ValueError, "found 2"):
            self.select([self.artifact(first), self.artifact(second)])

    def test_non_executable_artifact_fails_closed(self):
        binary = self.root / "target/debug/velnor-actions"
        binary.parent.mkdir(parents=True)
        binary.write_text("not executable\n")
        binary.chmod(0o600)
        with self.assertRaisesRegex(ValueError, "is not executable"):
            self.select([self.artifact(binary)])


if __name__ == "__main__":
    unittest.main()
