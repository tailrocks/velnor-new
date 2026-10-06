"""Actual filesystem primitive proof; synthetic metadata fixtures are labeled."""
import hashlib
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from source_archive_inventory import _inventory, _inventory_numbered, source_archive_inventory
from source_archive_inventory_common import InventoryError
from source_archive_inventory_fs import read_file, root_descriptor
from source_archive_inventory_leaf import HardlinkInventory, entry_at


class ActualPrimitiveTests(unittest.TestCase):
    """No filesystem metadata discovery is mocked in these tests."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.file = self.root / "file"
        self.file.write_bytes(b"actual stable payload")
        self.descriptor = root_descriptor(str(self.root))

    def tearDown(self):
        os.close(self.descriptor)
        self.temporary.cleanup()

    def metadata(self, name):
        return os.stat(name, dir_fd=self.descriptor, follow_symlinks=False)

    def group(self, names):
        group = HardlinkInventory()
        for name in names:
            metadata = self.metadata(name)
            digest, _size = read_file(self.descriptor, name, metadata, True)
            group.observe(metadata, digest, str(self.root / name))
        return group

    def test_actual_rooted_regular_bytes_have_expected_sha_and_count(self):
        expected = hashlib.sha256(b"actual stable payload").hexdigest(), 21
        self.assertEqual(read_file(self.descriptor, "file", self.metadata("file")), expected)

    def test_actual_hardlinked_bytes_require_complete_group_admission(self):
        os.link(self.file, self.root / "alias")
        with self.assertRaisesRegex(InventoryError, "payload_file_changed_or_hardlinked"):
            read_file(self.descriptor, "file", self.metadata("file"))

    def test_actual_complete_hardlink_group_finishes(self):
        os.link(self.file, self.root / "alias")
        self.group(("file", "alias")).finish()

    def test_actual_external_or_missing_hardlink_group_rejected(self):
        os.link(self.file, self.root / "alias")
        with self.assertRaisesRegex(InventoryError, "payload_hardlink_external_or_missing"):
            self.group(("file",)).finish()

    def test_actual_group_mutation_rejected_after_observation(self):
        os.link(self.file, self.root / "alias")
        group = self.group(("file", "alias"))
        self.file.write_bytes(b"changed")
        with self.assertRaisesRegex(InventoryError, "payload_hardlink_changed"):
            group.finish()

    def test_actual_appledouble_bytes_rejected(self):
        header = struct.pack(">II16sH", 0x00051607, 0x00020000, b"\0" * 16, 1)
        self.file.write_bytes(header + struct.pack(">III", 2, 38, 1) + b"x")
        with self.assertRaisesRegex(InventoryError, "payload_appledouble_metadata"):
            read_file(self.descriptor, "file", self.metadata("file"))

    def test_actual_fifo_rejected_without_blocking(self):
        os.mkfifo(self.root / "fifo")
        with self.assertRaisesRegex(InventoryError, "payload_file_changed_or_hardlinked"):
            read_file(self.descriptor, "fifo", self.metadata("fifo"))

    def test_actual_mutation_during_sink_consumption_rejected(self):
        with self.assertRaisesRegex(InventoryError, "payload_file_changed"):
            read_file(self.descriptor, "file", self.metadata("file"),
                      consume=lambda _chunk: self.file.write_bytes(b"changed"))

    def test_actual_special_mode_never_admitted(self):
        self.file.chmod(0o4700)
        # Metadata-bearing hosts reject even earlier than special-mode admission.
        with self.assertRaisesRegex(InventoryError, "payload_(unsupported_metadata|special_mode)"):
            entry_at(self.descriptor, "file", "file", str(self.file))


class SyntheticMetadataCompatibilityTests(unittest.TestCase):
    """Metadata bypass proves unit encoding only, never archive qualification."""
    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_synthetic_metadata_fixture_flattened_layout_and_optional_missing(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            live, stage = base / "live", base / "stage"
            (live / "a").mkdir(parents=True)
            (live / "b").mkdir()
            (live / "a" / "file").write_bytes(b"payload")
            os.link(live / "a" / "file", live / "b" / "alias")
            (stage / "roots" / "0").mkdir(parents=True)
            (stage / "roots" / "1").mkdir()
            (stage / "roots" / "0" / "file").write_bytes(b"payload")
            (stage / "roots" / "1" / "alias").write_bytes(b"payload")
            roots, optional = ("a", "b", "optional/file"), ("optional/file",)
            original = _inventory(str(live), roots, optional)
            self.assertEqual(original, _inventory_numbered(str(stage), roots, optional))
            self.assertEqual((original.files, original.bytes), (2, 14))
            self.assertEqual(original.digest, hashlib.sha256(original.canonical_bytes).hexdigest())
            self.assertIn(b'"kind":"missing"', original.canonical_bytes)

    def test_no_caller_object_can_activate_unqualified_projection(self):
        for context, capability in ((None, None), ({"schema": 2}, object()),
                                    (object(), {"qualified": True})):
            with self.assertRaisesRegex(InventoryError, "source_archive_projection_unqualified"):
                source_archive_inventory(context, capability)

    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_synthetic_metadata_archive_rejects_symlink_hardlink_aliases(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "tools").mkdir()
            (root / "tools" / "file").write_bytes(b"payload")
            (root / "tools" / "link").symlink_to("file")
            os.link(root / "tools" / "link", root / "tools" / "alias", follow_symlinks=False)
            with self.assertRaisesRegex(InventoryError, "payload_symlink_hardlink_unsupported"):
                _inventory(str(root), ("tools",))


if __name__ == "__main__":
    unittest.main()
