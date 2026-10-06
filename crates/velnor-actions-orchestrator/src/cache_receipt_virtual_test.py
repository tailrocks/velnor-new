"""Virtual inventory uses only immutable root order and logical links."""
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
from cache_receipt_manifest import inventory_exact_roots
from cache_receipt_common import ColdReceipt
from cache_receipt_virtual import inventory_quarantine


@patch("source_archive_inventory_leaf.metadata_records", return_value=[])
class VirtualTests(unittest.TestCase):
    def test_original_and_numbered_roots_have_identical_full_logical_inventory(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            producer, quarantine = base / "producer", base / "quarantine"
            (producer / "mise").mkdir(parents=True)
            (producer / "mise" / "gh").write_bytes(b"verified executable")
            (producer / "cargo" / "bin").mkdir(parents=True)
            (producer / "cargo" / "bin" / "link").symlink_to("../../mise/gh")
            (producer / "cargo" / "credentials").write_bytes(b"never exported")
            (quarantine / "roots").mkdir(parents=True)
            shutil.copytree(producer / "mise", quarantine / "roots" / "0", symlinks=True)
            shutil.copytree(producer / "cargo" / "bin", quarantine / "roots" / "1", symlinks=True)
            (quarantine / "archive-admission.json").write_text('{"roots":["attacker"]}')
            roots = ("mise", "cargo/bin", "cargo/.crates.toml")
            optional = ("cargo/.crates.toml",)
            expected = inventory_exact_roots(producer, roots, optional)
            actual = inventory_quarantine(quarantine, roots, optional)
            self.assertEqual(actual, expected)
            entries = json.loads(actual)["entries"]
            self.assertFalse(any(entry["path"] == "cargo" for entry in entries))
            self.assertEqual(json.loads(actual)["schema"], 2)
            (quarantine / "roots" / "1" / "link").unlink()
            (quarantine / "roots" / "1" / "link").symlink_to("../../credentials")
            with self.assertRaisesRegex(ColdReceipt, "payload_symlink_uncontained"):
                inventory_quarantine(quarantine, roots, optional)

    def test_index_order_extra_indices_required_absence_and_complete_children(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "roots").mkdir()
            (root / "roots" / "0").write_bytes(b"first")
            (root / "roots" / "1").mkdir()
            (root / "roots" / "1" / "child").write_bytes(b"second")
            initial = inventory_quarantine(root, ("a", "b"))
            swapped = inventory_quarantine(root, ("b", "a"))
            self.assertNotEqual(initial, swapped)
            (root / "roots" / "1" / "additional").write_bytes(b"all children bound")
            self.assertNotEqual(initial, inventory_quarantine(root, ("a", "b")))
            (root / "roots" / "2").write_bytes(b"unknown")
            with self.assertRaisesRegex(ColdReceipt, "quarantine_unknown_index"):
                inventory_quarantine(root, ("a", "b"))
            (root / "roots" / "2").unlink()
            (root / "roots" / "0").unlink()
            with self.assertRaisesRegex(ColdReceipt, "quarantine_required_root_missing"):
                inventory_quarantine(root, ("a", "b"))

    def test_envelope_and_root_symlinks_never_followed(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "outside").mkdir()
            (root / "roots").symlink_to(root / "outside")
            with self.assertRaisesRegex(ColdReceipt, "quarantine_inventory_unavailable"):
                inventory_quarantine(root, ("a",))
            (root / "roots").unlink()
            (root / "roots").mkdir()
            (root / "roots" / "0").symlink_to(str(root / "outside"))
            with self.assertRaisesRegex(ColdReceipt, "payload_symlink_escape"):
                inventory_quarantine(root, ("a",))
            (root / "roots" / "0").unlink()
            os.mkfifo(root / "roots" / "0")
            with self.assertRaisesRegex(ColdReceipt, "payload_special_entry"):
                inventory_quarantine(root, ("a",))

    def test_directory_depth_is_bounded_before_recursive_descent(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            producer = root / "producer"
            quarantine = root / "quarantine"
            (producer / "payload").mkdir(parents=True)
            (quarantine / "roots" / "0").mkdir(parents=True)
            for base in (producer / "payload", quarantine / "roots" / "0"):
                descriptor = os.open(base, os.O_RDONLY | os.O_DIRECTORY)
                try:
                    for _ in range(140):
                        os.mkdir("a", dir_fd=descriptor)
                        child = os.open("a", os.O_RDONLY | os.O_DIRECTORY, dir_fd=descriptor)
                        os.close(descriptor)
                        descriptor = child
                finally:
                    os.close(descriptor)
            with self.assertRaisesRegex(ColdReceipt, "payload_path_limit"):
                inventory_exact_roots(producer, ("payload",))
            with self.assertRaisesRegex(ColdReceipt, "payload_path_limit"):
                inventory_quarantine(quarantine, ("payload",))


if __name__ == "__main__":
    unittest.main()
