"""Pure same-root runner integration; actual fresh filesystem states, fake workloads."""
import importlib.util
import json
from pathlib import Path
import sys
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


PREVIOUS = module("same_root_test_helpers", "test_join.py")
RUNNER = module("same_root_runner", "run_same_root.py")
JOIN = module("same_root_join", "join_same_root.py")


class SameRootTests(PREVIOUS.RunnerJoinTests):
    def actual_same_root(self):
        args, source = self.prepare_runner()
        args.active_root = self.root / "exclusive-active"
        state_number = 0
        original_acquire = RUNNER.acquire_root

        def acquire(path, number):
            nonlocal state_number
            state_number = number
            return original_acquire(path, number)

        def fake(argv, cwd, env, logs, name):
            values = [str(value) for value in argv]
            if values[1:3] != ["cache", "export"] or state_number == 1:
                return self.fake_execute(argv, cwd, env, logs, name)
            stdout, stderr = logs / (name + ".stdout"), logs / (name + ".stderr")
            stdout.write_text('{"exported":false}')
            stderr.write_bytes(b"")
            return dict(argv=values, cwd=str(cwd), environment=dict(env), returncode=0,
                        stdout=RUNNER.BASE.artifact(stdout), stderr=RUNNER.BASE.artifact(stderr))

        record = dict(schema=1, scope=JOIN.BASE.SCOPE, execution_kind="local_same_root",
            run_attempt_id=args.group, status="failed", runs=[], same_root_states=[], active_root=str(args.active_root),
            native_authority=None, native_abi=None, hosted_t01_t03=None,
            host=dict(system="synthetic", release="synthetic", machine="synthetic"), limitations=[])
        lock = args.active_root.with_name(args.active_root.name + ".ownership")
        lock.mkdir()
        record["exclusive_ownership"] = dict(**RUNNER.allocation(lock), nonce="a" * 32)
        with patch.object(RUNNER.BASE, "ROOT", source), patch.object(RUNNER.BASE.BIND, "ROOT", source), \
                patch.object(RUNNER.BASE, "execute", side_effect=fake), \
                patch.object(RUNNER, "acquire_root", side_effect=acquire):
            RUNNER.run_all(args, record)
        lock.rmdir()
        record["exclusive_ownership"]["released_ns"] = time.monotonic_ns()
        record["artifacts"] = [RUNNER.BASE.artifact(path) for path in sorted(args.output.rglob("*"))
            if path.is_file() and ("logs" in str(path.parent) or "-reports" in str(path.parent))]
        return record, RUNNER.BASE.artifact(source / "manifest.json")

    def test_three_actual_fresh_states_join_retained_native_bytes(self):
        record, manifest = self.actual_same_root()
        raw = json.dumps(record).encode()
        descriptor = self.artifact("same-root-execution.json", raw)
        result = JOIN.join(descriptor, manifest)
        self.assertEqual(result.status, "unknown")
        self.assertIsNone(result.native_authority)
        self.assertEqual(result.lifetime.generic_taskwide_status, "unknown")
        self.assertEqual(len(result.observed_reports), 6)
        self.assertEqual(len({run["cwd"] for run in record["runs"]}), 1)
        self.assertFalse(Path(record["active_root"]).exists())
        for report in result.observed_reports:
            original = report["original_report_artifact"]
            self.assertFalse(Path(original["path"]).exists())
            self.assertEqual(original["sha256"], report["artifact"]["sha256"])
            self.assertEqual(len(report["opaque_admission_artifacts"]), 5)
            self.assertIsNone(report["statistics"]["measurement"]["link_wall_ns"])
        self.assertTrue(all(state["destroyed"] and state["pristine_entries"] == []
                            for state in result.physical_states))

    def test_retained_byte_mutation_rejects(self):
        record, manifest = self.actual_same_root()
        witness = record["same_root_states"][0]["retained_artifacts"][0]
        Path(witness["retained"]["path"]).write_bytes(b"altered archival bytes")
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "artifact bytes differ"):
            JOIN.join(descriptor, manifest)

    def test_reused_mutable_cache_rejects(self):
        record, manifest = self.actual_same_root()
        record["same_root_states"][1]["directories"]["cache"]["initial_entries"] = ["native.db"]
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "lifecycle receipt differs|mutable state reused"):
            JOIN.join(descriptor, manifest)

    def test_overlapping_lifecycle_rejects(self):
        record, manifest = self.actual_same_root()
        record["same_root_states"][1]["allocation"]["created_ns"] = 1
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "lifecycle overlaps"):
            JOIN.join(descriptor, manifest)

    def test_inode_reuse_does_not_fabricate_or_block_freshness(self):
        record, manifest = self.actual_same_root()
        allocation = record["same_root_states"][0]["allocation"]
        for state in record["same_root_states"][1:]:
            state["allocation"]["device"] = allocation["device"]
            state["allocation"]["inode"] = allocation["inode"]
        JOIN.lifecycle(record["same_root_states"], Path(record["active_root"]), record["exclusive_ownership"])

    def test_unlisted_archived_file_rejects(self):
        record, manifest = self.actual_same_root()
        root = Path(record["same_root_states"][0]["retained_root"])
        (root / "unlisted-native.db").write_bytes(b"unlisted mutable state")
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "physical archive coverage differs"):
            JOIN.join(descriptor, manifest)

    def test_omitted_original_witness_rejects(self):
        record, manifest = self.actual_same_root()
        record["same_root_states"][0]["retained_artifacts"].pop()
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "physical archive coverage differs"):
            JOIN.join(descriptor, manifest)

    def test_original_boolean_size_rejects(self):
        with self.assertRaisesRegex(ValueError, "original size/digest invalid"):
            JOIN.origin(dict(path="/gone/state/file", size=False, sha256="0" * 64))

    def test_released_ownership_lock_must_be_absent(self):
        record, manifest = self.actual_same_root()
        lock = Path(record["exclusive_ownership"]["path"])
        lock.mkdir()
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "exclusive lease remains"):
            JOIN.join(descriptor, manifest)
        lock.rmdir()
        lock.symlink_to(self.root / "missing-lease")
        with self.assertRaisesRegex(ValueError, "exclusive lease remains"):
            JOIN.join(descriptor, manifest)

    def test_native_flag_cannot_promote_same_root(self):
        record, manifest = self.actual_same_root()
        record["native_authority"] = True
        descriptor = self.artifact("same-root-execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "parsed native authority flag"):
            JOIN.join(descriptor, manifest)


if __name__ == "__main__":
    unittest.main()
