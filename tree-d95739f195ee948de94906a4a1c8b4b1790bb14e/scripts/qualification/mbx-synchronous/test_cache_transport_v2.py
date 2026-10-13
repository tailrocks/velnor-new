"""Native public API regressions; emitted delta never proves workspace usefulness."""
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("public_transport_test", Path(__file__).with_name("cache_transport_v2.py"))
API = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(API)


def native_export(exported=True):
    return dict(version=2, budget_refused=False, snapshot_budget_bytes=100,
        exported=exported, actions=1, objects=2, bytes=10, emitted_bundle_useful_delta=exported,
        workspace_usefulness=dict(status="unavailable", reason="scheduler_validity_not_proven"),
        delta=dict(new_action_results=0, changed_action_results=0, new_predictions=0,
                   new_workspace_variants=1, changed_workspace_variants=0), semantic_digest="0" * 64,
        workspace_comparison="relative_path_type_content_mode_symlink_target",
        workspace_comparison_exclusions=["effective_build_root/.rustc_info.json"],
        workspace_transport_scope="recorded_target_and_build_directories", workspace_capture="captured",
        workspace_capture_unavailable_reason=None, workspace_persistence_verified=False,
        qualification=API.CAPTURED_QUALIFICATION)


class TransportTests(unittest.TestCase):
    def validate(self, operation, value):
        return API.validate(operation, json.dumps(value).encode())

    def test_v2_emitted_bundle_delta_stays_separate_from_unavailable_workspace(self):
        for emitted in (True, False):
            value = native_export()
            value["emitted_bundle_useful_delta"] = emitted
            self.assertEqual(self.validate("export", value), value)
            self.assertEqual(value["workspace_usefulness"]["status"], "unavailable")
            self.assertFalse(value["workspace_persistence_verified"])

    def test_old_export_versions_and_usefulness_aliases_reject(self):
        for name, extra in (("version", 1), ("useful_delta", True), ("workspace_useful_delta", True),
                            ("workspace_usefulness_unavailable_reason", "owner_proof_unavailable")):
            with self.subTest(name=name):
                value = native_export()
                value[name] = extra
                with self.assertRaises(ValueError):
                    self.validate("export", value)

    def test_missing_extra_fields_boolean_counts_and_positive_authority_reject(self):
        for name, replacement in (("actions", True), ("bytes", -1), ("objects", 1 << 64),
                                  ("workspace_persistence_verified", True), ("workspace_usefulness", {"status":"available","reason":"scheduler_validity_not_proven"}),
                                  ("delta", {"new_action_results":0}), ("semantic_digest", "invalid")):
            with self.subTest(name=name):
                value = native_export()
                value[name] = replacement
                with self.assertRaises(ValueError):
                    self.validate("export", value)
        value = native_export()
        del value["emitted_bundle_useful_delta"]
        with self.assertRaises(ValueError):
            self.validate("export", value)

    def test_closed_capture_reason_pairs_and_unavailable_diagnostics(self):
        for capture, reason in API.CAPTURE_REASON.items():
            value = native_export()
            value["workspace_capture"] = capture
            value["workspace_usefulness"]["reason"] = reason
            value["workspace_capture_unavailable_reason"] = "observed diagnostic text"
            value["qualification"] = API.CAPTURED_QUALIFICATION if capture == "captured" else API.UNAVAILABLE_QUALIFICATION
            self.assertEqual(self.validate("export", value), value)
        value["workspace_usefulness"]["reason"] = "new_unreviewed_reason"
        with self.assertRaises(ValueError):
            self.validate("export", value)

    def test_exact_budget_refusal_schema_remains_failed_transport_observation(self):
        value = dict(version=2, exported=False, budget_refused=True, snapshot_budget_bytes=10,
                     logical_closure_bytes=11, qualification=API.BUDGET_QUALIFICATION)
        self.assertEqual(self.validate("export", value), value)
        value["emitted_bundle_useful_delta"] = False
        with self.assertRaises(ValueError):
            self.validate("export", value)

    def test_import_and_comparison_stay_native_version_one(self):
        imported = dict(version=1, actions=1, objects=1, bytes=10, comparison_state_recorded=True,
                        workspace_restored=False, workspace_restore="skipped_incompatible")
        self.assertEqual(self.validate("import", imported), imported)
        compared = dict(version=1, valid=True, empty=True)
        self.assertEqual(self.validate("comparison-state", compared), compared)
        for operation, value in (("import", imported), ("comparison-state", compared)):
            value["version"] = 2
            with self.assertRaises(ValueError):
                self.validate(operation, value)
        imported["version"] = 1
        imported["workspace_restored"] = True
        with self.assertRaises(ValueError):
            self.validate("import", imported)

    def test_duplicate_nonfinite_oversized_and_unknown_reports_reject(self):
        for raw in (b'{"version":2,"version":1}', b'{"version":NaN}', b'[]'):
            with self.assertRaises(ValueError):
                API.validate("export", raw)
        with self.assertRaises(ValueError):
            API.validate("export", b" " * (API.MAX_BYTES + 1))
        with self.assertRaises(ValueError):
            self.validate("internal-cache-ledger", {})


if __name__ == "__main__":
    unittest.main()
