"""Behavioral tests for the shared Tofu directory owner."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("tofu_root_owner.sh")
ROOT_KEY = "dir-737461636b732f767063"
DIGEST_LOCATOR = "b3-" + "a" * 64
LONG_SOURCE_ROOT = "/".join("r" * 200 for _ in range(4))
LONG_ROOT_KEY = "dir-" + LONG_SOURCE_ROOT.encode().hex()


class RootOwnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()

    def run_owner(self, directory, root_key=ROOT_KEY, hook=None):
        hook = hook or self.base / "owner-hook"
        command = (
            f". {shlex.quote(str(SCRIPT))}; "
            f'own_directory "$2"; '
            f"touch {shlex.quote(str(hook))}"
        )
        return subprocess.run(
            ["/bin/sh", "-c", command, "tofu-root-owner", root_key, str(directory)],
            check=False,
            capture_output=True,
            env={"PATH": "/usr/bin:/bin"},
        )

    def marker(self, directory):
        return Path(directory) / ".velnor-root-key"

    def assert_owned(self, directory, root_key):
        marker = self.marker(directory)
        self.assertTrue(marker.is_file())
        self.assertFalse(marker.is_symlink())
        self.assertEqual(marker.read_bytes(), root_key.encode())
        self.assertNotIn(b"\n", marker.read_bytes())
        self.assertEqual(marker.stat().st_nlink, 1)

    def assert_rejected_before_hook(self, result, hook):
        self.assertNotEqual(result.returncode, 0, result.stderr.decode())
        self.assertFalse(hook.exists())

    def test_fresh_empty_directory_writes_exact_marker(self):
        directory = self.base / "fresh"
        hook = self.base / "fresh-hook"

        result = self.run_owner(directory, hook=hook)

        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertTrue(hook.is_file())
        self.assert_owned(directory, ROOT_KEY)

    def test_matching_marker_reuse_succeeds_without_rewrite(self):
        directory = self.base / "reuse"
        first_hook = self.base / "first-hook"
        second_hook = self.base / "second-hook"
        self.assertEqual(self.run_owner(directory, hook=first_hook).returncode, 0)
        marker = self.marker(directory)
        before = marker.stat()

        result = self.run_owner(directory, hook=second_hook)

        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertTrue(second_hook.is_file())
        self.assert_owned(directory, ROOT_KEY)
        after = marker.stat()
        self.assertEqual((after.st_ino, after.st_nlink), (before.st_ino, 1))

    def test_foreign_marker_rejects_before_hook_or_write(self):
        directory = self.base / "foreign"
        directory.mkdir()
        marker = self.marker(directory)
        marker.write_bytes(b"dir-foreign")
        before = marker.read_bytes()
        hook = self.base / "foreign-hook"

        result = self.run_owner(directory, hook=hook)

        self.assert_rejected_before_hook(result, hook)
        self.assertEqual(marker.read_bytes(), before)

    def test_absent_marker_in_nonempty_directory_rejects(self):
        directory = self.base / "nonempty"
        directory.mkdir()
        payload = directory / "unrelated"
        payload.write_bytes(b"preserve")
        hook = self.base / "nonempty-hook"

        result = self.run_owner(directory, hook=hook)

        self.assert_rejected_before_hook(result, hook)
        self.assertFalse(self.marker(directory).exists())
        self.assertEqual(payload.read_bytes(), b"preserve")

    def test_symlink_marker_rejects_before_hook_or_write(self):
        directory = self.base / "symlink"
        directory.mkdir()
        target = self.base / "symlink-target"
        target.write_bytes(ROOT_KEY.encode())
        marker = self.marker(directory)
        marker.symlink_to(target)
        hook = self.base / "symlink-hook"

        result = self.run_owner(directory, hook=hook)

        self.assert_rejected_before_hook(result, hook)
        self.assertTrue(marker.is_symlink())
        self.assertEqual(target.read_bytes(), ROOT_KEY.encode())

    def test_hardlink_marker_rejects_before_hook_or_write(self):
        directory = self.base / "hardlink"
        directory.mkdir()
        target = self.base / "hardlink-target"
        target.write_bytes(ROOT_KEY.encode())
        marker = self.marker(directory)
        os.link(target, marker)
        self.assertEqual(marker.stat().st_nlink, 2)
        hook = self.base / "hardlink-hook"

        result = self.run_owner(directory, hook=hook)

        self.assert_rejected_before_hook(result, hook)
        self.assertEqual(marker.read_bytes(), ROOT_KEY.encode())
        self.assertEqual(marker.stat().st_nlink, 2)

    def test_long_source_root_uses_bounded_locator_and_long_exact_marker(self):
        source_root = self.base.joinpath(*(["r" * 200] * 4))
        source_root.mkdir(parents=True)
        (source_root / "main.tf").write_bytes(b"terraform {}\n")
        output = self.base / "tofu-data" / DIGEST_LOCATOR
        hook = self.base / "long-hook"

        self.assertEqual(len(LONG_SOURCE_ROOT.encode()), 803)
        self.assertEqual(len(DIGEST_LOCATOR.encode()), 67)
        self.assertEqual(len(LONG_ROOT_KEY.encode()), 1610)
        result = self.run_owner(output, LONG_ROOT_KEY, hook)

        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertTrue(hook.is_file())
        self.assert_owned(output, LONG_ROOT_KEY)
        self.assertGreaterEqual(self.marker(output).stat().st_size, 1606)


if __name__ == "__main__":
    unittest.main()
