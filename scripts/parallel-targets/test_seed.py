"""Filesystem and resource regression tests for the native cold-lane initializer."""

import contextlib
import importlib.util
import io
import os
import pathlib
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("seed", pathlib.Path(__file__).with_name("seed.py"))
seed = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(seed)


class SeedTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        self.target = self.root / "target"
        self.debug = self.target / "debug"
        self.debug.mkdir(parents=True)
        (self.debug / "deps").mkdir()
        self.source = self.debug / "deps" / "sample.rlib"
        self.source.write_bytes(b"native artifact\x00")
        os.utime(self.source, ns=(1234567890000000000, 1234567890000000000))
        (self.debug / ".cargo-lock").touch()
        (self.target / ".rustc_info.json").write_text("{}")
        self.environment = mock.patch.dict(os.environ, {"GITHUB_WORKSPACE": str(self.root)})
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def run_seed(self, *lanes, target="target"):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            seed.seed(target, list(lanes or ("velnor-clippy", "velnor-doc")))
        self.assertEqual(list(self.target.glob(".velnor-seed-*")), [])
        return output.getvalue()

    def test_old_python_skips_without_touching_the_target(self):
        with mock.patch.object(seed.sys, "version_info", (3, 10, 12)):
            output = self.run_seed()
        self.assertIn("Use Python 3.11 or later.", output)
        self.assertFalse((self.target / "velnor-clippy").exists())

    def test_complete_independent_copies_preserve_bytes_mtime_and_exclude_lock(self):
        self.run_seed()
        for lane in ("velnor-clippy", "velnor-doc"):
            copied = self.target / lane / "debug" / "deps" / self.source.name
            self.assertEqual(copied.read_bytes(), self.source.read_bytes())
            self.assertEqual(copied.stat().st_mtime_ns, self.source.stat().st_mtime_ns)
            self.assertNotEqual(copied.stat().st_ino, self.source.stat().st_ino)
            self.assertEqual(copied.stat().st_nlink, 1)
            self.assertFalse((copied.parent.parent / ".cargo-lock").exists())
            copied.write_bytes(b"changed")
        self.assertEqual(self.source.read_bytes(), b"native artifact\x00")

    def test_existing_empty_and_warm_lanes_remain_untouched(self):
        empty = self.target / "velnor-clippy"
        empty.mkdir()
        warm = self.target / "velnor-doc"
        warm.mkdir()
        (warm / "foreign").write_text("preserved")
        before = empty.stat().st_ino, warm.stat().st_ino
        self.run_seed()
        self.assertEqual(before, (empty.stat().st_ino, warm.stat().st_ino))
        self.assertEqual(list(empty.iterdir()), [])
        self.assertEqual((warm / "foreign").read_text(), "preserved")

    def test_only_cargos_debug_lock_is_excluded_from_complete_payload(self):
        output = self.debug / "build" / "package" / "out"
        output.mkdir(parents=True)
        (output / ".cargo-lock").write_bytes(b"legitimate build-script payload")
        self.run_seed()
        for lane in ("velnor-clippy", "velnor-doc"):
            copied = self.target / lane / "debug" / "build" / "package" / "out" / ".cargo-lock"
            self.assertEqual(copied.read_bytes(), b"legitimate build-script payload")
            self.assertFalse((self.target / lane / "debug" / ".cargo-lock").exists())

    def test_scoped_workspace_uses_only_its_target(self):
        scoped = self.root / "crates" / "workspace"
        scoped.mkdir(parents=True)
        self.target.rename(scoped / "target")
        self.target = scoped / "target"
        self.run_seed(target="crates/workspace/target")
        self.assertTrue((self.target / "velnor-doc" / "debug" / "deps" / "sample.rlib").is_file())
        self.assertFalse((self.root / "target").exists())

    def test_source_symlinks_and_special_files_fail_closed(self):
        for name, create in (("alias", lambda path: path.symlink_to(self.source)),
                             ("fifo", os.mkfifo)):
            with self.subTest(name=name):
                path = self.debug / name
                create(path)
                try:
                    with self.assertRaises(ValueError):
                        self.run_seed()
                    self.assertFalse((self.target / "velnor-clippy").exists())
                finally:
                    path.unlink()

    def test_existing_lane_symlink_or_file_fails_closed(self):
        lane = self.target / "velnor-doc"
        lane.symlink_to(self.debug, target_is_directory=True)
        with self.assertRaises(ValueError):
            self.run_seed()
        lane.unlink()
        lane.write_text("foreign")
        with self.assertRaises(ValueError):
            self.run_seed()
        self.assertEqual(lane.read_text(), "foreign")

    def test_target_symlink_and_path_traversal_rejected(self):
        (self.root / "alias").symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(OSError):
            self.run_seed(target="alias/target")
        for path in ("../target", "/target", "target/../target", "other"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.run_seed(target=path)

    def test_byte_disk_and_elapsed_limits_leave_cold_targets(self):
        for patch in (mock.patch.object(seed, "MAX_BYTES", 1),
                      mock.patch.object(seed.shutil, "disk_usage", return_value=mock.Mock(free=0)),
                      mock.patch.object(seed, "SECONDS", -1)):
            with patch:
                self.assertIn("optional seed skipped", self.run_seed())
            self.assertFalse((self.target / "velnor-clippy").exists())

    def test_partial_copy_failure_removes_only_owned_staging(self):
        original = seed.Snapshot.copy_file

        def interrupted(snapshot, source, destination, expected):
            original(snapshot, source, destination, expected)
            raise seed.Budget("injected interruption")

        with mock.patch.object(seed.Snapshot, "copy_file", interrupted):
            self.assertIn("injected interruption", self.run_seed())
        self.assertEqual(self.source.read_bytes(), b"native artifact\x00")
        self.assertFalse((self.target / "velnor-clippy").exists())

    def test_source_mutation_fails_closed_without_admission(self):
        original = seed.Snapshot.copy_file

        def mutated(snapshot, source, destination, expected):
            self.source.write_bytes(b"mutated")
            original(snapshot, source, destination, expected)

        with mock.patch.object(seed.Snapshot, "copy_file", mutated):
            with self.assertRaises(ValueError):
                self.run_seed()
        self.assertFalse((self.target / "velnor-clippy").exists())

    def test_atomic_admission_never_replaces_concurrent_foreign_directory(self):
        original = seed.rename_exclusive

        def race(source, destination, parent):
            (self.target / destination).mkdir()
            (self.target / destination / "foreign").write_text("preserved")
            original(source, destination, parent)

        with mock.patch.object(seed, "rename_exclusive", race):
            with self.assertRaises(OSError):
                self.run_seed("velnor-clippy")
        self.assertEqual((self.target / "velnor-clippy" / "foreign").read_text(), "preserved")

    def test_growth_during_read_cannot_write_beyond_admitted_size(self):
        expected = self.source.stat()
        destination = self.root / "copy"
        original = seed.os.read
        changed = False

        def growing(descriptor, size):
            nonlocal changed
            if not changed:
                changed = True
                with self.source.open("ab") as output:
                    output.write(b"unadmitted growth")
            return original(descriptor, size)

        with self.source.open("rb") as incoming, destination.open("xb") as outgoing:
            with mock.patch.object(seed.os, "read", growing), self.assertRaises(ValueError):
                seed.Snapshot().copy_file(incoming.fileno(), outgoing.fileno(), expected)
        self.assertLessEqual(destination.stat().st_size, expected.st_size)

    def test_regular_to_fifo_swap_is_nonblocking_and_rejected(self):
        original = seed.os.open

        def swapped(path, flags, *args, **kwargs):
            if path == self.source.name and flags & os.O_NONBLOCK:
                self.source.unlink()
                os.mkfifo(self.source)
            return original(path, flags, *args, **kwargs)

        with mock.patch.object(seed.os, "open", swapped):
            with self.assertRaises(ValueError):
                self.run_seed()
        self.assertFalse((self.target / "velnor-clippy").exists())

    def test_staging_replacement_cannot_receive_writes_or_be_admitted(self):
        original = seed.copy_snapshot

        def replaced(snapshot, source, stage):
            name = next(self.target.glob(".velnor-seed-*"))
            name.rename(self.root / "held-stage")
            name.mkdir()
            (name / "foreign").write_text("preserved")
            original(snapshot, source, stage)

        with mock.patch.object(seed, "copy_snapshot", replaced):
            with self.assertRaisesRegex(ValueError, "refusing cleanup"):
                self.run_seed("velnor-clippy")
        replacement = next(self.target.glob(".velnor-seed-*"))
        self.assertEqual(list(path.name for path in replacement.iterdir()), ["foreign"])
        self.assertEqual((replacement / "foreign").read_text(), "preserved")
        self.assertFalse((self.target / "velnor-clippy").exists())

    def test_unavailable_native_rename_skips_without_partial_admission(self):
        with mock.patch.object(seed.ctypes, "CDLL", return_value=object()):
            self.assertIn("atomic exclusive rename unsupported", self.run_seed())
        self.assertFalse((self.target / "velnor-clippy").exists())
        self.assertFalse((self.target / "velnor-doc").exists())

    def test_completed_first_lane_survives_second_lane_budget_failure(self):
        original = seed.rename_exclusive
        calls = 0

        def second_unavailable(source, destination, parent):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise seed.Budget("injected second lane limit")
            original(source, destination, parent)

        with mock.patch.object(seed, "rename_exclusive", second_unavailable):
            output = self.run_seed()
        self.assertIn("seeded velnor-clippy", output)
        self.assertIn("optional seed skipped", output)
        self.assertTrue((self.target / "velnor-clippy" / "debug" / "deps" / self.source.name).exists())
        self.assertFalse((self.target / "velnor-doc").exists())

    def test_stage_swap_inside_rename_is_detected_before_success(self):
        original = seed.rename_exclusive

        def replaced(source, destination, parent):
            path = self.target / source
            path.rename(self.root / "held-stage")
            path.mkdir()
            (path / "foreign").write_text("preserved")
            original(source, destination, parent)

        with mock.patch.object(seed, "rename_exclusive", replaced):
            with self.assertRaisesRegex(ValueError, "admitted lane identity changed"):
                self.run_seed("velnor-clippy")
        self.assertEqual((self.target / "velnor-clippy" / "foreign").read_text(), "preserved")


if __name__ == "__main__":
    unittest.main()
