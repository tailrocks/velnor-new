"""Both actual local drivers flow through strict V2 routing and unchanged typed joins."""
import importlib.util
import json
from pathlib import Path
import sys
import time
from unittest.mock import patch
import unittest

ROOT = Path(__file__).resolve().parent


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


HELPER = module("v2_join_helpers", "test_join.py")
RUNTIME = module("v2_join_runtime", "run_v2.py")
JOIN = module("v2_join_wrapper", "join_v2.py")
PUBLIC = module("v2_join_public_fixture", "test_cache_transport_v2.py")


class JoinV2Tests(HELPER.RunnerJoinTests):
    def pipeline(self, layout):
        args, source = self.prepare_runner()
        filename = "run.py" if layout == "relocated" else "run_same_root.py"
        driver = module("v2_actual_driver_" + layout, filename)
        base = driver if layout == "relocated" else driver.BASE
        args.active_root = self.root / "active-root"
        record = dict(schema=1, scope=JOIN.RELOCATED.SCOPE, execution_kind="local" if layout == "relocated" else "local_same_root",
            run_attempt_id=args.group, status="failed", runs=[], native_authority=None, native_abi=None,
            hosted_t01_t03=None, host=dict(system="synthetic", release="synthetic", machine="synthetic"), limitations=[])
        lock = None
        if layout == "same-root":
            record.update(active_root=str(args.active_root), same_root_states=[])
            lock = args.active_root.with_name(args.active_root.name + ".ownership")
            lock.mkdir()
            record["exclusive_ownership"] = dict(**driver.allocation(lock), nonce="b" * 32)
        export_number = 0

        def execute(argv, cwd, env, logs, name):
            nonlocal export_number
            values = [str(value) for value in argv]
            operation = RUNTIME.owned_transport(values)
            if operation == "export":
                export_number += 1
                exported = export_number <= 2
                if exported:
                    Path(values[3]).mkdir()
                    (Path(values[3]) / "native-object").write_bytes(b"supported immutable bundle " + bytes([export_number]))
                stdout, stderr = logs / (name + ".stdout"), logs / (name + ".stderr")
                public = PUBLIC.native_export(exported)
                stdout.write_text(json.dumps(public))
                stderr.write_bytes(b"")
                return dict(argv=values, cwd=str(cwd), environment=dict(env), returncode=0,
                            stdout=base.artifact(stdout), stderr=base.artifact(stderr))
            observed = self.fake_execute(argv, cwd, env, logs, name)
            if operation == "import":
                public = dict(version=1, actions=1, objects=1, bytes=10, comparison_state_recorded=True,
                              workspace_restored=False, workspace_restore="skipped_incompatible")
            elif operation == "comparison-state":
                public = dict(version=1, valid=True, empty=True)
            else:
                return observed
            stdout = Path(observed["stdout"]["path"])
            stdout.write_text(json.dumps(public))
            observed["stdout"] = base.artifact(stdout)
            return observed

        with patch.object(base, "ROOT", source), patch.object(base.BIND, "ROOT", source), \
                patch.object(base, "execute", side_effect=execute), RUNTIME.strict_execution(base, JOIN.PROTOCOL):
            driver.run_all(args, record)
        if lock is not None:
            lock.rmdir()
            record["exclusive_ownership"]["released_ns"] = time.monotonic_ns()
        record["artifacts"] = [base.artifact(path) for path in sorted(args.output.rglob("*"))
                              if path.is_file() and not path.is_symlink() and
                              ("logs" in str(path.parent) or "-reports" in str(path.parent))]
        execution = base.write_json(args.output / "execution.json", record)
        RUNTIME.bind_envelope(base, args.output, layout, ROOT / "cache_transport_v2.py", ROOT / filename)
        envelope = base.artifact(args.output / "execution-v2.json")
        manifest = base.artifact(source / "manifest.json")
        return envelope, manifest, execution

    def assert_join_pipeline(self, layout):
        envelope, manifest, execution = self.pipeline(layout)
        if layout == "same-root":
            manifest_value = JOIN.RELOCATED.document(JOIN.RELOCATED.artifact(manifest))
            fixture_locks = [item for item in manifest_value["fixture"]["files"]
                             if item["path"] == "Cargo.lock.fixture"]
            self.assertEqual(len(fixture_locks), 1)
            record = JOIN.RELOCATED.document(
                JOIN.RELOCATED.artifact(execution, JOIN.SAME.MAX_RECORD_BYTES))
            for state in record["same_root_states"]:
                workspace = Path(record["runs"][state["id"] - 1]["cwd"])
                witnesses = [item for item in state["retained_artifacts"]
                             if item["path"] == str(workspace / "Cargo.lock")]
                self.assertEqual(len(witnesses), 1)
                self.assertEqual(witnesses[0]["sha256"], fixture_locks[0]["sha256"])
                self.assertFalse(any(item["path"] == str(workspace / "Cargo.lock.fixture")
                                     for item in state["retained_artifacts"]))
        result = JOIN.join(envelope, manifest, execution["sha256"], JOIN.RELOCATED.sha((ROOT / "cache_transport_v2.py").read_bytes()))
        self.assertEqual(result["status"], "unknown")
        self.assertIsNone(result["native_authority"])
        self.assertEqual(len(result["original_join"]["observed_reports"]), 6)
        observations = result["observed_transport"]
        self.assertEqual(len(observations), 6)
        exports = [value["native_report"] for value in observations if value["operation"] == "export"]
        self.assertEqual([value["exported"] for value in exports], [True, True, False])
        self.assertTrue(all(value["version"] == 2 and value["workspace_usefulness"]["status"] == "unavailable"
                            and value["workspace_persistence_verified"] is False for value in exports))
        self.assertTrue(all(value["statistics"]["measurement"]["link_wall_ns"] is None
                            for value in result["original_join"]["observed_reports"]))
        self.assertEqual(result["original_join"]["lifetime"]["generic_taskwide_status"], "unknown")

    def test_actual_relocated_driver_routes_v2_and_joins_unchanged_metrics(self):
        self.assert_join_pipeline("relocated")

    def test_same_root_v2_join_accepts_materialized_fixture_lock_alias(self):
        self.assert_join_pipeline("same-root")

    def test_envelope_api_bool_and_independent_execution_mismatch_reject(self):
        envelope, manifest, execution = self.pipeline("relocated")
        value = json.loads(Path(envelope["path"]).read_bytes())
        value["transport_api"]["import"] = True
        descriptor = self.artifact("forged-envelope.json", json.dumps(value).encode())
        parser_sha = JOIN.RELOCATED.sha((ROOT / "cache_transport_v2.py").read_bytes())
        with self.assertRaisesRegex(ValueError, "transport API differs"):
            JOIN.join(descriptor, manifest, execution["sha256"], parser_sha)
        with self.assertRaisesRegex(ValueError, "independent execution identity differs"):
            JOIN.join(envelope, manifest, "0" * 64, parser_sha)


if __name__ == "__main__":
    unittest.main()
