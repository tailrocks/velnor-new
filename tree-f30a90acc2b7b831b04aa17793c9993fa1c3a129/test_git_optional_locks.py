"""Optional locks protect status reads without disabling explicit Git writes."""

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "stage_owned_tool_source", Path(__file__).with_name("stage-owned-tool-source.py"))
STAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STAGE)


class OptionalLocks(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-optional-locks-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.executable = shutil.which("git", path=os.defpath)
        self.assertIsNotNone(self.executable)
        self.environment = {"PATH": os.defpath, "HOME": str(self.root), "LC_ALL": "C",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_SYSTEM": os.devnull,
            "GIT_CONFIG_GLOBAL": os.devnull, "GIT_OPTIONAL_LOCKS": "0"}
        self.git("init", "--quiet", "--template=")
        self.file = self.source / "file"
        self.file.write_bytes(b"unchanged\n")
        self.git("add", "--", "file")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                 "commit", "--quiet", "-m", "fixture")

    def git(self, *arguments, optional_locks="0"):
        environment = dict(self.environment, GIT_OPTIONAL_LOCKS=optional_locks)
        return subprocess.run([self.executable, "-C", str(self.source),
            "-c", "core.hooksPath=" + os.devnull, "-c", "core.fsmonitor=false",
            "-c", "commit.gpgsign=false", *arguments], env=environment,
            check=True, capture_output=True).stdout

    def touch(self):
        metadata = self.file.stat()
        os.utime(self.file, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 2_000_000_000))

    def test_stage_status_preserves_source_index_with_hostile_parent_control(self):
        index = self.source / ".git/index"
        before = index.read_bytes()
        self.touch()
        with patch.dict(os.environ, self.environment | {"GIT_OPTIONAL_LOCKS": "1"}, clear=True):
            self.assertEqual(STAGE.git(self.source, "status", "--porcelain"), b"")
        self.assertEqual(index.read_bytes(), before)
        self.assertEqual(self.git("status", "--porcelain", optional_locks="1"), b"")
        self.assertNotEqual(index.read_bytes(), before)

    def test_optional_locks_allow_explicit_index_commit_and_clone_writes(self):
        self.file.write_bytes(b"changed\n")
        self.git("add", "--", "file")
        self.assertEqual(self.git("show", ":file"), b"changed\n")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                 "commit", "--quiet", "-m", "changed")
        destination = self.root / "clone"
        self.git("clone", "--local", "--no-hardlinks", "--template=", "--",
                 str(self.source), str(destination))
        self.assertEqual((destination / "file").read_bytes(), b"changed\n")
        self.assertTrue((destination / ".git/index").is_file())


if __name__ == "__main__":
    unittest.main()
