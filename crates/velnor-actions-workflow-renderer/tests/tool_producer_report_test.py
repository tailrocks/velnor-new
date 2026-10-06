"""Terminal evidence never turns an incomplete executable cache into availability."""
from pathlib import Path
import json
import re
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "tools/tool_producer_report.py"
ASSEMBLER = Path(__file__).parents[1] / "src/cache_tool_report.rs"
# Execute the exact pinned Rust source envelope, substituting only compiled bytes.
TEMPLATE = re.search(r'"set -eu\\n/usr/bin/python3[^"\n]*"',
                     ASSEMBLER.read_text()).group(0)
SOURCE = json.loads(TEMPLATE).replace("{python}", SCRIPT.read_text())
DIGEST = "a" * 64
IDENTITY = "qualified-linux-image-version"
MATCHED = IDENTITY + "-snapshot-" + DIGEST + "-123-1"


class ToolProducerReport(unittest.TestCase):
    def observe(self, **controls):
        env = {
            "VELNOR_TOOL_IDENTITY": IDENTITY,
            "VELNOR_TOOL_BEFORE_AVAILABLE": "true",
            "VELNOR_TOOL_AFTER_AVAILABLE": "true",
            "VELNOR_TOOL_BEFORE_DIGEST": DIGEST,
            "VELNOR_TOOL_AFTER_DIGEST": DIGEST,
            "VELNOR_TOOL_BEFORE_OUTCOME": "success",
            "VELNOR_TOOL_AFTER_OUTCOME": "success",
            "VELNOR_TOOL_DESCRIPTOR_IDENTITY": "qualified-linux",
            "VELNOR_TOOL_INSTALL_OUTCOME": "success",
            "VELNOR_TOOL_VERIFIED": "true",
            "VELNOR_TOOL_SAVE_OUTCOME": "skipped",
            "VELNOR_TOOL_PUBLICATION_EXACT_HIT": "false",
            "VELNOR_TOOL_PUBLICATION_OUTCOME": "skipped",
            "VELNOR_TOOL_PUBLICATION_MATCHED_KEY": "",
            "VELNOR_TOOL_PUBLICATION_EXPECTED_KEY": MATCHED,
            "VELNOR_TOOL_RESTORE_OUTCOME": "success",
            "VELNOR_TOOL_MATCHED_KEY": "",
            "VELNOR_TOOL_CHANGED": "true",
        }
        env.update(controls)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            env["GITHUB_OUTPUT"] = str(output)
            subprocess.run(["/bin/sh", "-c", SOURCE], env=env, check=True)
            return dict(line.split("=", 1) for line in output.read_text().splitlines())

    def test_verified_successful_publication(self):
        report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success",
                              VELNOR_TOOL_PUBLICATION_OUTCOME="success",
                              VELNOR_TOOL_PUBLICATION_MATCHED_KEY=MATCHED,
                              VELNOR_TOOL_PUBLICATION_EXACT_HIT="true")
        self.assertEqual(report["cache_available"], "true")
        self.assertEqual(report["verified"], "true")
        self.assertEqual(report["error"], "NONE")

    def test_descriptor_and_resolved_transport_identity_are_bound(self):
        report = self.observe()
        self.assertEqual(report["descriptoridentity"], "qualified-linux")
        self.assertEqual(report["toolidentity"], "qualified-linux-image-version")

    def test_unchanged_verified_warm_candidate(self):
        report = self.observe(VELNOR_TOOL_MATCHED_KEY=MATCHED,
                              VELNOR_TOOL_CHANGED="false")
        self.assertEqual(report["cache_available"], "true")

    def test_missing_transport_is_not_available(self):
        report = self.observe()
        self.assertEqual(report["cache_available"], "false")
        self.assertEqual(report["error"], "CACHE_NOT_PUBLISHED")

    def test_repaired_candidate_without_publication_is_not_available(self):
        report = self.observe(VELNOR_TOOL_MATCHED_KEY="corrupt-candidate")
        self.assertEqual(report["cache_available"], "false")

    def test_publication_failure_survives_as_terminal_evidence(self):
        report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="failure")
        self.assertEqual(report["error"], "CACHE_TRANSPORT_FAILED")

    def test_install_verification_is_mandatory(self):
        for change in ({"VELNOR_TOOL_VERIFIED": "false"},
                       {"VELNOR_TOOL_INSTALL_OUTCOME": "failure"},
                       {"VELNOR_TOOL_INSTALL_OUTCOME": "cancelled"}):
            report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success", **change)
            self.assertEqual(report["cache_available"], "false")
            self.assertEqual(report["verified"], "false")
            self.assertEqual(report["error"], "PREPARATION_FAILED")

    def test_snapshot_failure_suppresses_even_successful_save(self):
        for name in ("BEFORE_AVAILABLE", "AFTER_AVAILABLE", "BEFORE_DIGEST",
                     "AFTER_DIGEST", "BEFORE_OUTCOME", "AFTER_OUTCOME", "CHANGED"):
            for bad in ("", "false", "failure", "cancelled"):
                if name == "CHANGED" and bad == "false":
                    continue
                report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success",
                                      **{"VELNOR_TOOL_" + name: bad})
                self.assertEqual(report["verified"], "false", (name, bad))
                self.assertEqual(report["cache_available"], "false", (name, bad))
                self.assertEqual(report["error"], "PREPARATION_FAILED")

    def test_foreign_or_malformed_warm_keys_are_not_available(self):
        keys = ("candidate", "other-" + MATCHED, MATCHED + "-extra",
                MATCHED.replace(DIGEST, "b" * 64),
                MATCHED.replace("-123-1", "-0-1"),
                MATCHED.replace("-123-1", "-123-0"),
                MATCHED.replace("-123-1", "-01-1"))
        for key in keys:
            report = self.observe(VELNOR_TOOL_MATCHED_KEY=key,
                                  VELNOR_TOOL_CHANGED="false")
            self.assertEqual(report["cache_available"], "false", key)

    def test_unchanged_claim_with_different_digests_fails(self):
        report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success",
                              VELNOR_TOOL_BEFORE_DIGEST="b" * 64,
                              VELNOR_TOOL_CHANGED="false")
        self.assertEqual(report["error"], "PREPARATION_FAILED")

    def test_warm_availability_survives_failed_or_cancelled_save(self):
        for outcome in ("failure", "cancelled"):
            report = self.observe(VELNOR_TOOL_MATCHED_KEY=MATCHED,
                                  VELNOR_TOOL_CHANGED="false",
                                  VELNOR_TOOL_SAVE_OUTCOME=outcome)
            self.assertEqual(report["cache_available"], "true")
            self.assertEqual(report["verified"], "true")
            self.assertEqual(report["error"], "CACHE_TRANSPORT_FAILED")

    def test_failed_restore_cannot_claim_warm_availability(self):
        for outcome in ("failure", "cancelled", "skipped", ""):
            report = self.observe(VELNOR_TOOL_MATCHED_KEY=MATCHED,
                                  VELNOR_TOOL_CHANGED="false",
                                  VELNOR_TOOL_RESTORE_OUTCOME=outcome)
            self.assertEqual(report["cache_available"], "false")

    def test_unknown_save_outcome_preserves_warm_state_and_failure(self):
        for outcome in ("", "unknown"):
            report = self.observe(VELNOR_TOOL_SAVE_OUTCOME=outcome,
                                  VELNOR_TOOL_MATCHED_KEY=MATCHED,
                                  VELNOR_TOOL_CHANGED="false")
            self.assertEqual(report["cache_available"], "true")
            self.assertEqual(report["error"], "CACHE_TRANSPORT_FAILED")

    def test_save_success_requires_exact_service_publication(self):
        for controls in ({}, {"VELNOR_TOOL_PUBLICATION_OUTCOME": "failure"},
                         {"VELNOR_TOOL_PUBLICATION_OUTCOME": "success"},
                         {"VELNOR_TOOL_PUBLICATION_OUTCOME": "success",
                          "VELNOR_TOOL_PUBLICATION_MATCHED_KEY": "foreign-key"}):
            report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success", **controls)
            self.assertEqual(report["cache_available"], "false")
            self.assertEqual(report["error"], "CACHE_TRANSPORT_UNAVAILABLE")

    def test_unconfirmed_save_preserves_exact_warm_availability(self):
        report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success",
                              VELNOR_TOOL_MATCHED_KEY=MATCHED,
                              VELNOR_TOOL_CHANGED="false")
        self.assertEqual(report["cache_available"], "true")
        self.assertEqual(report["error"], "CACHE_TRANSPORT_FAILED")

    def test_publication_exact_hit_proof_is_mandatory(self):
        for hit in ("", "false", "unknown"):
            report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="success",
                                  VELNOR_TOOL_PUBLICATION_OUTCOME="success",
                                  VELNOR_TOOL_PUBLICATION_MATCHED_KEY=MATCHED,
                                  VELNOR_TOOL_PUBLICATION_EXACT_HIT=hit)
            self.assertEqual(report["cache_available"], "false")
            self.assertEqual(report["error"], "CACHE_TRANSPORT_UNAVAILABLE")

    def test_exact_publication_available_despite_failed_save(self):
        report = self.observe(VELNOR_TOOL_SAVE_OUTCOME="failure",
                              VELNOR_TOOL_PUBLICATION_OUTCOME="success",
                              VELNOR_TOOL_PUBLICATION_MATCHED_KEY=MATCHED,
                              VELNOR_TOOL_PUBLICATION_EXACT_HIT="true")
        self.assertEqual(report["cache_available"], "true")
        self.assertEqual(report["error"], "CACHE_TRANSPORT_FAILED")

    def test_missing_snapshot_cannot_claim_warm_availability(self):
        report = self.observe(VELNOR_TOOL_MATCHED_KEY="candidate", VELNOR_TOOL_CHANGED="")
        self.assertEqual(report["cache_available"], "false")


if __name__ == "__main__":
    unittest.main()
