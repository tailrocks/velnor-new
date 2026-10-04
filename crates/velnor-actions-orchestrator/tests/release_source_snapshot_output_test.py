"""Fixed output body checks; runner capability admission belongs to its compiler."""
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release_source_snapshot_entry_test as entry_fixture


ROOT = Path(__file__).resolve().parents[1] / "src"


class SourceSnapshotOutputTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.output = self.directory / "runner-output"
        self.output.write_bytes(b"previous=value\n")

    def load(self, path):
        namespace = {"__name__": "velnor_release_compiled"}
        source = ROOT / "release_reconcile_common.py"
        exec(compile(source.read_bytes(), str(source), "exec"), namespace)
        with patch.dict(os.environ, {}, clear=True):
            if path is not None:
                os.environ["GITHUB_OUTPUT"] = str(path)
            source = ROOT / "release_source_snapshot_output.py"
            exec(compile(source.read_bytes(), str(source), "exec"), namespace)
        return namespace

    def test_exact_fixed_keys_append_and_emit_once(self):
        namespace = self.load(self.output)
        writer = namespace["write_source_snapshot_outputs"]
        writer("a" * 64, "b" * 40, "c" * 40)
        self.assertEqual(self.output.read_text(), "previous=value\n" +
                         "source-snapshot-blob-sha256=" + "a" * 64 + "\n" +
                         "source-commit-sha=" + "b" * 40 + "\n" +
                         "source-tree-sha=" + "c" * 40 + "\n")
        with self.assertRaisesRegex(namespace["ReconcileError"], "repeated"):
            writer("a" * 64, "b" * 40, "c" * 40)

    def test_missing_or_relative_path_denied_at_module_load(self):
        for path in (None, "relative-output"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.load(path)

    def test_missing_channel_never_created(self):
        path = self.directory / "absent"
        with self.assertRaises(FileNotFoundError):
            self.load(path)
        self.assertFalse(path.exists())

    def test_symlink_channel_never_followed(self):
        path = self.directory / "link"
        path.symlink_to(self.output)
        with self.assertRaises(OSError):
            self.load(path)
        self.assertEqual(self.output.read_text(), "previous=value\n")

    def test_nonregular_channel_denied_without_blocking(self):
        fifo = self.directory / "fifo"
        os.mkfifo(fifo)
        for path in (self.directory, fifo):
            with self.subTest(path=path), self.assertRaises((OSError, ValueError)):
                self.load(path)

    def test_malformed_values_never_emit_and_close_channel(self):
        for values in ((True, "b" * 40, "c" * 40),
                       ("a" * 64 + "\nextra=value", "b" * 40, "c" * 40),
                       ("a" * 64, "B" * 40, "c" * 40),
                       ("a" * 64, "b" * 40, 1)):
            with self.subTest(values=values):
                namespace = self.load(self.output)
                writer = namespace["write_source_snapshot_outputs"]
                with self.assertRaises(namespace["ReconcileError"]):
                    writer(*values)
                self.assertEqual(self.output.read_text(), "previous=value\n")
                with self.assertRaisesRegex(namespace["ReconcileError"], "repeated"):
                    writer("a" * 64, "b" * 40, "c" * 40)

    def test_real_entry_and_codec_use_fixed_writer(self):
        fixture = entry_fixture.SourceSnapshotEntryTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        source = ROOT / "release_source_snapshot_output.py"
        with patch.dict(os.environ, {"GITHUB_OUTPUT": str(self.output)}):
            exec(compile(source.read_bytes(), str(source), "exec"), fixture.ns)
        fixture.invoke()
        raw = (fixture.directory / "release-source-snapshot/snapshot.zip").read_bytes()
        self.assertIn("source-snapshot-blob-sha256=" + hashlib.sha256(raw).hexdigest() + "\n",
                      self.output.read_text())
        self.assertEqual(fixture.outputs, [])


if __name__ == "__main__":
    unittest.main()
