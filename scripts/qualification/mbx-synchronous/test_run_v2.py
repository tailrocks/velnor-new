"""V2 runtime routing regressions; synthetic processes never establish authority."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import importlib.util

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("v2_run", ROOT / "run_v2.py")
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)
VALIDATOR = RUN.load("v2_validator_test", ROOT / "cache_transport_v2.py")
BASE = RUN.load("v2_base_test", ROOT / "run.py")
HELPERS = RUN.load("same_root_test_helpers", ROOT / "test_run_same_root.py")


def export_report(exported):
    return dict(version=2, budget_refused=False, snapshot_budget_bytes=None,
                exported=exported, actions=1, objects=1, bytes=8,
                emitted_bundle_useful_delta=exported,
                workspace_usefulness=dict(status="unavailable", reason="scheduler_validity_not_proven"),
                delta=None, semantic_digest="0" * 64,
                workspace_comparison="relative_path_type_content_mode_symlink_target",
                workspace_comparison_exclusions=["effective_build_root/.rustc_info.json"],
                workspace_transport_scope="recorded_target_and_build_directories",
                workspace_capture="captured", workspace_capture_unavailable_reason=None,
                workspace_persistence_verified=False, qualification=VALIDATOR.CAPTURED_QUALIFICATION)


class V2RuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()

    def native_record(self, value):
        stdout = self.root / "native.stdout"
        stderr = self.root / "native.stderr"
        stdout.write_text(json.dumps(value))
        stderr.write_bytes(b"")
        return dict(argv=["/owned/mbx", "cache", "export"], cwd=str(self.root),
                    environment={}, returncode=0, stdout=BASE.artifact(stdout), stderr=BASE.artifact(stderr))

    def test_legacy_export_rejects_before_bundle_routing_and_retains_original_bytes(self):
        value = export_report(True)
        value["version"] = 1
        record = self.native_record(value)
        raw = Path(record["stdout"]["path"]).read_bytes()
        with patch.object(BASE, "execute", return_value=record):
            original = BASE.execute
            with RUN.strict_execution(BASE, VALIDATOR):
                with self.assertRaisesRegex(ValueError, "strict V2 export"):
                    BASE.execute(record["argv"], self.root, {}, self.root, "export")
            self.assertIs(BASE.execute, original)
        self.assertEqual(Path(record["stdout"]["path"]).read_bytes(), raw)
        retained = json.loads((self.root / "export.transport-validation.json").read_bytes())
        self.assertEqual(retained["command"], record)
        self.assertIsNone(retained["native_authority"])

    def test_oversized_native_stdout_is_rejected_without_rewriting_raw_bytes(self):
        record = self.native_record(export_report(True))
        original = Path(record["stdout"]["path"]).read_bytes()
        with patch.object(BASE, "execute", return_value=record), \
             patch.object(VALIDATOR, "MAX_BYTES", 64):
            with RUN.strict_execution(BASE, VALIDATOR):
                with self.assertRaisesRegex(ValueError, "exceeds bound"):
                    BASE.execute(record["argv"], self.root, {}, self.root, "export")
        self.assertEqual(Path(record["stdout"]["path"]).read_bytes(), original)
        self.assertGreater(record["stdout"]["size"], 64)

    def test_valid_v2_preserves_typed_workspace_unknown(self):
        record = self.native_record(export_report(True))
        with patch.object(BASE, "execute", return_value=record):
            with RUN.strict_execution(BASE, VALIDATOR):
                actual = BASE.execute(record["argv"], self.root, {}, self.root, "export")
        self.assertIs(actual, record)
        raw = json.loads(Path(actual["stdout"]["path"]).read_bytes())
        self.assertEqual(raw["workspace_usefulness"]["status"], "unavailable")
        self.assertFalse(raw["workspace_persistence_verified"])

    def test_envelope_binds_unmodified_driver_record(self):
        original = dict(status="failed", native_authority=None, native_abi=None)
        execution = BASE.write_json(self.root / "execution.json", original)
        driver = ROOT / "run.py"
        RUN.bind_envelope(BASE, self.root, "relocated", ROOT / "cache_transport_v2.py", driver)
        value = json.loads((self.root / "execution-v2.json").read_bytes())
        self.assertEqual(value["execution_record"], execution)
        self.assertEqual(value["transport_api"], {"export": 2, "import": 1, "comparison": 1})
        self.assertEqual(json.loads(Path(execution["path"]).read_bytes()), original)

    def assert_pipeline(self, filename):
        driver = RUN.load("v2_pipeline_" + filename, ROOT / filename)
        base = driver.BASE if filename == "run_same_root.py" else driver
        simulation = HELPERS.SameRootTests(methodName="test_retention_failure_preserves_original_state")
        simulation.setUp()
        self.addCleanup(simulation.doCleanups)
        args = simulation.protocol_arguments()
        simulation.operations, simulation.imported = [], []
        manifest = json.loads((ROOT / "manifest.json").read_bytes())
        record = dict(runs=[], same_root_states=[], native_authority=None)

        def execute(argv, *arguments):
            observed = simulation.protocol_execute(argv, *arguments)
            operation = RUN.owned_transport(argv)
            if operation == "export":
                old = json.loads(Path(observed["stdout"]["path"]).read_bytes())
                public = export_report(old["exported"])
            elif operation == "import":
                public = dict(version=1, actions=1, objects=1, bytes=8,
                              comparison_state_recorded=True, workspace_restored=False,
                              workspace_restore="skipped_incompatible")
            elif operation == "comparison-state":
                public = dict(version=1, valid=True, empty=True)
            else:
                return observed
            Path(observed["stdout"]["path"]).write_text(json.dumps(public))
            observed["stdout"] = base.artifact(Path(observed["stdout"]["path"]))
            return observed

        with patch.object(base.BIND, "verify", return_value=(manifest, "synthetic")), \
             patch.object(base, "tool_observations"), \
             patch.object(base, "execute", side_effect=execute):
            with RUN.strict_execution(base, VALIDATOR):
                driver.run_all(args, record)
        self.assertEqual(record["status"], "observed")
        self.assertEqual(sum(len(item["commands"]) for item in record["runs"]), 6)
        self.assertEqual(simulation.imported, [b"native export 1", b"native export 2"])
        self.assertIsNone(record["native_authority"])
        for item in record["runs"]:
            export = item["transport"][-1]["export_observed"]
            self.assertEqual(export["version"], 2)
            self.assertEqual(export["workspace_usefulness"]["status"], "unavailable")

    def test_actual_same_root_driver_three_states_pass_strict_public_transport(self):
        self.assert_pipeline("run_same_root.py")

    def test_actual_relocated_driver_three_states_pass_strict_public_transport(self):
        self.assert_pipeline("run.py")


if __name__ == "__main__":
    unittest.main()
