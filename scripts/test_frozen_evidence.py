# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("frozen", Path(__file__).with_name("check-frozen-evidence.py"))
frozen = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(frozen)

CONFIG = """rules:
  - id: no-absolute-local-paths
    paths:
      exclude:
        - ".alint.yml"
        - "docs/a.json"
  - id: other
    paths:
      exclude:
        - "docs/ignored.json"
"""


class FrozenEvidence(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "docs").mkdir()
        (self.root / "docs/a.json").write_text("{}")
        self.config = self.root / ".alint.yml"
        self.config.write_text(CONFIG)
        self.manifest = self.root / "manifest"
        frozen.write(self.root, self.config, self.manifest)

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self):
        return frozen.check(self.root, self.config, self.manifest)

    def test_untouched_evidence_passes(self):
        self.assertEqual(self.problems(), [])

    def test_edited_evidence_is_reported(self):
        (self.root / "docs/a.json").write_text('{"path": "edited"}')
        self.assertEqual(self.problems(), ["changed: docs/a.json"])

    def test_deleted_evidence_is_reported(self):
        (self.root / "docs/a.json").unlink()
        self.assertEqual(self.problems(), ["missing: docs/a.json"])

    def test_new_exemption_without_pin_is_reported(self):
        self.config.write_text(CONFIG.replace('        - "docs/a.json"\n', '        - "docs/a.json"\n        - "docs/b.json"\n'))
        self.assertEqual(self.problems(), ["exempt but not pinned: docs/b.json"])

    def test_unquoted_and_single_quoted_exemptions_are_read(self):
        self.config.write_text(CONFIG.replace('        - "docs/a.json"\n', "        - docs/a.json\n        - 'docs/b.json'\n        - docs/c.json # note\n"))
        self.assertEqual(
            self.problems(),
            ["exempt but not pinned: docs/b.json", "exempt but not pinned: docs/c.json"],
        )

    def test_flow_style_exemptions_are_read(self):
        self.config.write_text(
            CONFIG.replace(
                '      exclude:\n        - ".alint.yml"\n        - "docs/a.json"\n',
                '      exclude: [".alint.yml", "docs/a.json", \'docs/b.json\', docs/c.json]  # note\n',
            )
        )
        self.assertEqual(
            self.problems(),
            ["exempt but not pinned: docs/b.json", "exempt but not pinned: docs/c.json"],
        )

    def test_a_multi_line_flow_list_fails_closed(self):
        self.config.write_text(
            CONFIG.replace(
                '      exclude:\n        - ".alint.yml"\n        - "docs/a.json"\n',
                '      exclude: [".alint.yml",\n        "docs/a.json"]\n',
            )
        )
        self.assertEqual(self.problems(), ["unsupported multi-line exemption list in no-absolute-local-paths"])

    def test_include_lists_are_not_exemptions(self):
        self.config.write_text(CONFIG.replace('      exclude:\n', '      include:\n        - "**/*.md"\n      exclude:\n', 1))
        self.assertEqual(self.problems(), [])

    def test_a_symlink_to_pinned_bytes_is_reported(self):
        (self.root / "docs/copy.json").write_text("{}")
        (self.root / "docs/a.json").unlink()
        (self.root / "docs/a.json").symlink_to("copy.json")
        self.assertEqual(self.problems(), ["symlink: docs/a.json"])

    def test_pin_without_exemption_is_reported(self):
        self.config.write_text(CONFIG.replace('        - "docs/a.json"\n', ""))
        self.assertEqual(self.problems(), ["pinned but not exempt: docs/a.json"])


if __name__ == "__main__":
    unittest.main()
