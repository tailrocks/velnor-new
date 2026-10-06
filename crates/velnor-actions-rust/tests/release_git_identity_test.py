"""Rust-owned source checkout identity proof; retained for the Git test lane."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


RUST = Path(__file__).resolve().parents[1] / "src"
ORCHESTRATOR = RUST.parents[1] / "velnor-actions-orchestrator" / "src"
NAMESPACE = {"__name__": "git_identity_test"}
SOURCES = (
    (RUST, "release_source_validation.py"),
    (ORCHESTRATOR, "release_reconcile_common.py"),
    (RUST, "release_reconcile_cargo.py"),
    (RUST, "release_preflight_cargo.py"),
    (RUST, "release_package.py"),
)
for owner, filename in SOURCES:
    path = owner / filename
    exec(compile(path.read_text(), str(path), "exec"), NAMESPACE)


class GitIdentityTests(unittest.TestCase):
    def setUp(self):
        with patch.dict(os.environ, {"RELEASE_RUST_TOOLCHAIN": "1.98.1"}):
            self.environment = NAMESPACE["_clean_environment"]()
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.source, self.source_sha = self._repository(root, "source")
        external, self.wrong_sha = self._repository(root, "external")
        self.marker = root / "executed"
        executable = root / "fsmonitor"
        executable.write_text("#!/bin/sh\ntouch '" + str(self.marker) + "'\n")
        executable.chmod(0o755)
        config = root / "hostile.gitconfig"
        config.write_text("[core]\nfsmonitor = " + str(executable) + "\n")
        hostile = {
            "RELEASE_RUST_TOOLCHAIN": "1.98.1",
            "GIT_DIR": str(external / ".git"),
            "GIT_WORK_TREE": str(external),
            "GIT_CONFIG_GLOBAL": str(config),
            "GIT_CONFIG_SYSTEM": str(config),
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "core.fsmonitor",
            "GIT_CONFIG_VALUE_0": str(executable),
            "GIT_EXEC_PATH": str(root / "missing"),
        }
        selectors = patch.dict(os.environ, hostile)
        selectors.start()
        self.addCleanup(selectors.stop)
        self.addCleanup(lambda: self.assertFalse(self.marker.exists()))

    def _git(self, repository, *arguments):
        return subprocess.run(
            ["git", "-c", "user.name=Fixture", "-c",
             "user.email=fixture@example.com", *arguments],
            cwd=repository, env=self.environment, capture_output=True,
            text=True, check=True,
        ).stdout.strip()

    def _repository(self, root, name):
        repository = root / name
        repository.mkdir()
        self._git(repository, "init")
        (repository / "file").write_text(name)
        self._git(repository, "add", "file")
        self._git(repository, "commit", "-s", "-m",
                  "fixture\n\nCo-authored-by: Codex <codex@openai.com>")
        return repository, self._git(repository, "rev-parse", "HEAD")

    def _assert_dirty(self):
        with self.assertRaisesRegex(NAMESPACE["ReconcileError"], "^dirty_source$"):
            NAMESPACE["verify_source_checkout"](self.source, self.source_sha)

    def test_hostile_external_repo_and_git_config_selectors_are_ignored(self):
        NAMESPACE["verify_source_checkout"](self.source, self.source_sha)

    def test_wrong_head_is_rejected(self):
        with self.assertRaisesRegex(NAMESPACE["ReconcileError"],
                                    "^source_checkout_sha$"):
            NAMESPACE["verify_source_checkout"](self.source, self.wrong_sha)

    def test_untracked_file_is_rejected(self):
        (self.source / "untracked").write_text("dirty")
        self._assert_dirty()

    def test_modified_tracked_file_is_rejected(self):
        (self.source / "file").write_text("modified")
        self._assert_dirty()

    def test_staged_tracked_file_is_rejected(self):
        (self.source / "file").write_text("staged")
        self._git(self.source, "add", "file")
        self._assert_dirty()

    def test_deleted_tracked_file_is_rejected(self):
        (self.source / "file").unlink()
        self._assert_dirty()

    def test_clean_environment_retains_exact_allowed_git_values(self):
        cleaned = NAMESPACE["_clean_environment"]()
        self.assertEqual(
            {key: value for key, value in cleaned.items() if key.startswith("GIT_")},
            {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull},
        )


if __name__ == "__main__":
    unittest.main()
