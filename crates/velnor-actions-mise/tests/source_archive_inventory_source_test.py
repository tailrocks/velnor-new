"""Independent source mapping/framing checks; no Rust execution or FS writes."""
import ast
import hashlib
from pathlib import Path
import re
import struct
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "src"
SCHEMA = b"velnor-source-archive-inventory-template-v1"
NAMES = ["source_archive_inventory_common", "metadata_container",
         "source_archive_inventory_fs", "opaque_inventory_metadata",
         "source_archive_inventory_leaf", "source_archive_inventory_walk",
         "source_archive_inventory", "source_archive_inventory_original"]


def frame(value):
    return struct.pack(">Q", len(value)) + value


def source_template_digest(names):
    value = frame(SCHEMA) + struct.pack(">Q", len(names))
    for name in names:
        value += frame(name.encode()) + frame((SOURCE / (name + ".py")).read_bytes())
    return hashlib.sha256(value).hexdigest()


class SourceMappingFixture(unittest.TestCase):
    def test_rust_mapping_is_exact_and_single_owner(self):
        rust = (SOURCE / "source_archive_inventory.rs").read_text()
        pairs = re.findall(r'"([a-z_]+)",\s*include_str!\("([a-z_.]+)"\)', rust)
        self.assertEqual(pairs, [(name, name + ".py") for name in NAMES])
        self.assertIn('if program == InventorySourceProgram::OriginalFilesystem', rust)
        self.assertIn(SCHEMA.decode(), rust)
        self.assertIn('(sources.len() as u64).to_be_bytes()', rust)
        self.assertIn('(value.len() as u64).to_be_bytes()', rust)
        self.assertNotIn("Deserialize", rust)
        self.assertNotIn("SourceArchiveProjection", rust)

    def test_every_source_parses_and_internal_imports_close(self):
        for names in [NAMES[:7], NAMES]:
            for name in names:
                tree = ast.parse((SOURCE / (name + ".py")).read_text())
                imports = [node.module for node in ast.walk(tree)
                           if isinstance(node, ast.ImportFrom) and node.module]
                for module in imports:
                    if module.startswith("source_archive_inventory") or module in NAMES:
                        self.assertIn(module, names)

    def test_program_order_and_framing_change_identity(self):
        archive = source_template_digest(NAMES[:7])
        original = source_template_digest(NAMES)
        self.assertNotEqual(archive, original)
        self.assertNotEqual(archive, source_template_digest(list(reversed(NAMES[:7]))))
        self.assertEqual(archive, source_template_digest(NAMES[:7]))


if __name__ == "__main__":
    unittest.main()
