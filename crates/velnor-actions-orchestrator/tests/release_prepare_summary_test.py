"""Exact release-plz update stdout proof without process execution."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "summary_test"}
for name in ("release_reconcile_common.py", "release_prepare_bytes.py", "release_prepare_summary.py"):
    exec(compile((ROOT / name).read_text(), name, "exec"), NS)


class SummaryTests(unittest.TestCase):
    def parse(self, text, version="1.1.0", notes="feature"):
        return NS["preparation_summary"](text.encode(), {"demo": {"version": version, "notes": notes}},
                                          {"demo": "1.0.0"})["demo"]

    def test_transition_and_exact_compatible_outcome(self):
        value = self.parse("\n* `demo`: 1.0.0 -> 1.1.0 (✓ API compatible changes)\n\n")
        self.assertEqual(value, {"previous_version": "1.0.0", "semver_check": "compatible", "breaking_changes": ""})

    def test_skipped_is_only_unsuffixed_transition(self):
        self.assertEqual(self.parse("\n* `demo`: 1.0.0 -> 1.1.0\n")["semver_check"], "skipped")

    def test_equal_version_outcome_remains_unknown(self):
        self.assertEqual(self.parse("\n* `demo`: 1.1.0\n")["semver_check"], "unknown")

    def test_breaking_report_preserves_text_and_embedded_fence(self):
        report = "signature change\n```code```\n"
        text = "\n* `demo`: 1.0.0 -> 1.1.0 (⚠️ API breaking changes)\n\n### ⚠️ `demo` breaking changes\n\n```" + report + "```\n\n"
        value = self.parse(text)
        self.assertEqual(value["semver_check"], "incompatible")
        self.assertEqual(value["breaking_changes"], report)

    def test_breaking_equal_version_recovers_incompatible_outcome(self):
        text = "\n* `demo`: 1.1.0\n\n### ⚠️ `demo` breaking changes\n\n```report```\n"
        self.assertEqual(self.parse(text)["semver_check"], "incompatible")

    def test_unknown_duplicate_and_version_mismatch_fail(self):
        for text in ["* `other`: 1.0.0 -> 1.1.0", "* `demo`: 1.0.0 -> 9.9.9",
                     "* `demo`: 1.1.0\n* `demo`: 1.1.0", "unqualified summary",
                     "* `demo`: 1.0.0 -> 1.1.0 (⚠️ API breaking changes)"]:
            with self.assertRaises(NS["ReconcileError"]):
                self.parse(text)

    def test_missing_summary_update_fails_and_unchanged_noop_is_unknown(self):
        with self.assertRaises(NS["ReconcileError"]):
            self.parse("\n")
        value = self.parse("\n", "1.0.0", "")
        self.assertEqual(value["semver_check"], "unknown")


if __name__ == "__main__":
    unittest.main()
