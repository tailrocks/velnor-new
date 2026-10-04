"""Pure cold-manifest tests through the qualified original observer seam."""

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

MISE_SOURCE = Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))

from source_archive_inventory_common import InventoryError
from source_archive_inventory_fs import root_descriptor
import source_intent_cold_manifest as manifest
from source_intent_cold_common import ColdSourceIntent


ROOTS = (
    "mise",
    "cargo",
    "rustup-home",
    "rustup-bootstrap",
    "mise-config",
    "mise-system-config",
    "trusted-bin",
)


class FixtureContext:
    roots = ROOTS

    def __init__(self, root, seal):
        self.root = str(root)
        self.root_descriptor = root_descriptor(self.root)
        self._seal = seal
        self.checks = 0

    def require_current(self):
        self.checks += 1


def _layout(root):
    for leaf in ROOTS:
        (root / leaf).mkdir()
    (root / "mise" / "bin").mkdir()
    (root / "cargo" / "bin").mkdir()
    (root / "rustup-home" / "toolchains" / "cold" / "bin").mkdir(parents=True)
    (root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc").write_bytes(b"rustc")


class ColdManifestTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-cold-manifest-")
        self.root = Path(self.temporary.name).resolve()
        self.seal = object()
        self.context_type = patch.object(manifest, "FreshColdInstallationContext", FixtureContext)
        self.context_seal = patch.object(manifest, "_CONTEXT_SEAL", self.seal)
        self.context_type.start()
        self.context_seal.start()

    def tearDown(self):
        self.context_seal.stop()
        self.context_type.stop()
        self.temporary.cleanup()

    def observe(self):
        context = FixtureContext(self.root, self.seal)
        with patch("source_archive_inventory_leaf.metadata_records", return_value=[]):
            result = manifest.source_original_inventory(context)
        self.assertGreaterEqual(context.checks, 2)
        return result

    def test_absolute_in_root_target_is_raw_and_graph_checked(self):
        _layout(self.root)
        target = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc"
        link = self.root / "cargo" / "bin" / "cargo"
        raw = str(target)
        link.symlink_to(raw)
        data = json.loads(self.observe().canonical_bytes)
        entry = next(item for item in data["entries"] if item["path"] == "cargo/bin/cargo")
        self.assertEqual(data["schema"], 3)
        self.assertEqual(entry["target"], raw)
        self.assertIn("metadata", entry)

    def test_canonical_bytes_facade_returns_observation_only(self):
        _layout(self.root)
        context = FixtureContext(self.root, self.seal)
        with patch("source_archive_inventory_leaf.metadata_records", return_value=[]):
            data = manifest.inventory_cold_root(context)
        self.assertIs(type(data), bytes)
        self.assertEqual(json.loads(data)["schema"], 3)
        self.assertGreaterEqual(context.checks, 2)

    def test_complete_in_scope_hardlinks_preserve_topology(self):
        _layout(self.root)
        source = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc"
        alias = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc-alias"
        os.link(source, alias)
        data = json.loads(self.observe().canonical_bytes)
        links = [item for item in data["entries"] if item.get("hardlink")]
        self.assertEqual({item["hardlink"]["count"] for item in links}, {2})

    def test_outside_hardlink_alias_is_rejected(self):
        _layout(self.root)
        source = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc"
        outside = self.root.parent / "velnor-cold-outside-alias"
        os.link(source, outside)
        self.addCleanup(lambda: outside.unlink(missing_ok=True))
        with self.assertRaisesRegex(InventoryError, "payload_hardlink_external_or_missing"):
            self.observe()

    def test_outside_alias_and_transitive_escape_are_rejected(self):
        _layout(self.root)
        outside = self.root.parent / "velnor-cold-outside-target"
        outside.write_bytes(b"outside")
        self.addCleanup(lambda: outside.unlink(missing_ok=True))
        (self.root / "cargo" / "bin" / "escape").symlink_to(str(outside))
        with self.assertRaisesRegex(InventoryError, "payload_symlink_escape"):
            self.observe()

        (self.root / "cargo" / "bin" / "escape").unlink()
        target = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc"
        transitive = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc-link"
        transitive.symlink_to(str(target))
        (self.root / "cargo" / "bin" / "chain").symlink_to(str(transitive))
        with self.assertRaisesRegex(InventoryError, "payload_symlink_uncontained"):
            self.observe()

    def test_unsealed_or_data_context_cannot_reach_core(self):
        for context in (None, {}, object(), FixtureContext.__new__(FixtureContext)):
            with self.assertRaisesRegex(ColdSourceIntent, "cold_manifest_context_authority"):
                manifest.source_original_inventory(context)

    def test_symlink_ancestor_is_rejected(self):
        _layout(self.root)
        target = self.root / "rustup-home" / "toolchains" / "cold" / "bin" / "rustc"
        alias = self.root / "mise" / "rustup-alias"
        alias.symlink_to(self.root / "rustup-home")
        (self.root / "cargo" / "bin" / "ancestor").symlink_to(
            str(alias / "toolchains" / "cold" / "bin" / "rustc")
        )
        self.assertTrue(target.is_file())
        with self.assertRaisesRegex(InventoryError, "payload_symlink_ancestor"):
            self.observe()


if __name__ == "__main__":
    unittest.main()
