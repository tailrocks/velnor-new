"""Closed inode aliases admit ordinary Rustup proxies without outside links."""
import hashlib
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
from cache_receipt_manifest import HardlinkInventory, inventory_exact_roots
from cache_receipt_common import ColdReceipt
from cache_receipt_virtual import inventory_quarantine


class HardlinkTests(unittest.TestCase):
    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_complete_aliases_and_flattened_archive_copies_have_same_manifest(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            producer, quarantine = base / "producer", base / "quarantine"
            (producer / "cargo" / "bin").mkdir(parents=True)
            manager = producer / "cargo" / "bin" / "rustup"
            manager.write_bytes(b"manager")
            manager.chmod(0o755)
            os.link(manager, manager.with_name("cargo"))
            os.link(manager, manager.with_name("rustc"))
            (quarantine / "roots" / "0").mkdir(parents=True)
            for path in (manager, manager.with_name("cargo"), manager.with_name("rustc")):
                shutil.copy2(path, quarantine / "roots" / "0" / path.name)
            roots = ("cargo/bin",)
            expected = inventory_exact_roots(producer, roots)
            self.assertEqual(expected, inventory_quarantine(quarantine, roots))
            external = base / "unadmitted-manager"
            os.link(manager, external)
            with self.assertRaisesRegex(ColdReceipt, "payload_hardlink_external_or_missing"):
                inventory_exact_roots(producer, roots)

    def test_missing_alias_and_observed_count_mismatch_are_cold(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            first, second = root / "first", root / "second"
            first.write_bytes(b"content")
            os.link(first, second)
            group = HardlinkInventory()
            group.observe(os.lstat(first), "digest", first)
            with self.assertRaisesRegex(ColdReceipt, "payload_hardlink_external_or_missing"):
                group.finish()
            group.observe(os.lstat(second), "digest", second)
            group.finish()
            group.observe(os.lstat(second), "digest", second)
            with self.assertRaisesRegex(ColdReceipt, "payload_hardlink_external_or_missing"):
                group.finish()

    def test_mode_content_and_link_count_changes_are_cold(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            first, second = root / "first", root / "second"
            first.write_bytes(b"content")
            os.link(first, second)
            for mutation in (lambda: first.chmod(0o755), lambda: first.write_bytes(b"changed"),
                             lambda: os.link(first, root / "third")):
                first.chmod(0o644)
                first.write_bytes(b"content")
                group = HardlinkInventory()
                group.observe(os.lstat(first), hashlib.sha256(first.read_bytes()).hexdigest(), first)
                mutation()
                with self.assertRaisesRegex(ColdReceipt, "payload_hardlink_inconsistent"):
                    group.observe(os.lstat(second), hashlib.sha256(second.read_bytes()).hexdigest(), second)
                if (root / "third").exists():
                    (root / "third").unlink()

    def test_post_observation_mutation_is_rechecked(self):
        with tempfile.TemporaryDirectory() as temporary:
            file = Path(temporary).resolve() / "file"
            file.write_bytes(b"before")
            group = HardlinkInventory()
            group.observe(os.lstat(file), "digest", file)
            file.write_bytes(b"changed")
            with self.assertRaisesRegex(ColdReceipt, "payload_hardlink_changed"):
                group.finish()


if __name__ == "__main__":
    unittest.main()
