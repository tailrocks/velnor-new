"""Local lifecycle/retention regressions; no compiler execution or authority claims."""

import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("same_root", Path(__file__).with_name("run_same_root.py"))
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)


class SameRootTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.active = self.root / "active"

    def test_recreation_is_exclusive_and_pristine_without_inode_uniqueness_claim(self):
        states = []
        for number in (1, 2, 3):
            state = RUN.acquire_root(self.active, number)
            states.append(state)
            self.assertEqual(state["pristine_entries"], [])
            with self.assertRaisesRegex(ValueError, "already exists"):
                RUN.acquire_root(self.active, number)
            (self.active / "native-db").write_bytes(b"mutable state")
            RUN.archive_state(state, self.root / ("retained-" + str(number)))
            RUN.destroy_state(state)
            self.assertFalse(self.active.exists())
        self.assertEqual(len({item["allocation"]["nonce"] for item in states}), 3)
        self.assertTrue(all(item["allocation"]["created_ns"] <= item["retained_ns"]
                            <= item["destroyed_ns"] for item in states))
        self.assertTrue(all(states[i]["destroyed_ns"] <= states[i + 1]["allocation"]["created_ns"]
                            for i in (0, 1)))

    def test_retained_descriptor_preserves_original_path_and_raw_json(self):
        state = RUN.acquire_root(self.active, 1)
        path = self.active / "report.json"
        raw = json.dumps({"native_path": str(path), "native_authority": None}).encode()
        path.write_bytes(raw)
        original = RUN.BASE.artifact(path)
        record = dict(report=original, cwd=str(self.active))
        witnesses = RUN.archive_state(state, self.root / "retained")
        RUN.annotate_descriptors(record, witnesses)
        RUN.destroy_state(state)
        self.assertEqual(record["report"]["path"], str(path))
        self.assertEqual(record["cwd"], str(self.active))
        self.assertEqual(Path(record["report"]["retained"]["path"]).read_bytes(), raw)
        self.assertEqual(record["report"]["sha256"], record["report"]["retained"]["sha256"])
        self.assertIn("original_inode", witnesses[str(path)])
        self.assertIn("original_device", witnesses[str(path)])

    def test_unretained_or_replaced_root_cannot_be_destroyed(self):
        state = RUN.acquire_root(self.active, 1)
        with self.assertRaisesRegex(ValueError, "unretained"):
            RUN.destroy_state(state)
        RUN.archive_state(state, self.root / "retained")
        state["allocation"]["inode"] = -1
        with self.assertRaisesRegex(ValueError, "ownership changed"):
            RUN.destroy_state(state)
        self.assertTrue(self.active.exists())

    def test_only_immutable_bundle_bytes_cross_deleted_states(self):
        state = RUN.acquire_root(self.active, 1)
        bundle = self.active / "native-bundle"
        bundle.mkdir()
        (bundle / "owned-object").write_bytes(b"native export")
        inventory = RUN.BASE.write_json(self.active / "bundle-inventory.json", RUN.BASE.tree(bundle))
        (self.active / "mutable-target").write_bytes(b"not restored")
        args = argparse.Namespace(bundle_witnesses={str(bundle): inventory})
        witnesses = RUN.archive_state(state, self.root / "retained")
        retained = RUN.retain_bundle(bundle, state, witnesses, args)
        RUN.destroy_state(state)
        fresh = RUN.acquire_root(self.active, 2)
        self.assertEqual(list(self.active.iterdir()), [])
        self.assertEqual((retained / "owned-object").read_bytes(), b"native export")
        self.assertEqual(RUN.BASE.tree(retained), json.loads(Path(args.bundle_witnesses[str(retained)]["path"]).read_bytes()))
        self.assertTrue(fresh["pristine"])
        self.assertFalse((self.active / "mutable-target").exists())

    def protocol_arguments(self):
        seed = self.root / "registry-seed"
        archive = Path("registry/cache/id/itoa.crate")
        source = Path("registry/src/id/itoa")
        for relative in (archive, source / "src/lib.rs", Path("registry/index/id/config.json"),
                         Path("registry/index/id/.cache/it/oa/itoa")):
            target = seed / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"synthetic protocol input")
        tools = self.root / "tools"
        (tools / "bin").mkdir(parents=True)
        args = argparse.Namespace(active_root=self.active, output=self.root / "evidence",
                                  registry_home=seed, archive_relative=archive,
                                  source_relative=source, expected_manifest_sha256="synthetic",
                                  toolchain_root=tools, group="synthetic-protocol-attempt",
                                  prequalified_local=True, mbx_source_receipt=None,
                                  mbx_source_receipt_sha256=None, compiler_source_receipt=None,
                                  compiler_source_receipt_sha256=None)
        args.output.mkdir()
        for name in ("mbx", "cargo", "rustc"):
            tool = tools / "bin" / name
            tool.write_bytes(name.encode())
            setattr(args, name, tool)
            setattr(args, name + "_sha256", RUN.BASE.digest(tool))
        return args

    def protocol_execute(self, argv, cwd, env, logs, name):
        argv = [str(value) for value in argv]
        logs.mkdir(exist_ok=True)
        stdout = logs / (name + ".stdout")
        stderr = logs / (name + ".stderr")
        observed = {}
        if argv[1] == "cache":
            self.operations.append(argv[2])
            if argv[2] == "comparison-state":
                Path(argv[3]).write_text("{}")
            elif argv[2] == "import":
                copied = Path(argv[3])
                self.imported.append((copied / "object").read_bytes())
                RUN.shutil.rmtree(copied)
                Path(argv[argv.index("--comparison-state") + 1]).write_text("{}")
            else:
                number = self.operations.count("export")
                observed["exported"] = number < 3
                if observed["exported"]:
                    bundle = Path(argv[3])
                    bundle.mkdir()
                    (bundle / "object").write_bytes(("native export " + str(number)).encode())
        else:
            reports = Path(env["MBX_STATS_REPORT_DIR"])
            (reports / "session.json").write_text('{"completed": true}')
            hidden = reports / ".mbx-admissions-session"
            hidden.mkdir()
            (hidden / "closed").write_bytes(b"opaque native ledger")
        stdout.write_text(json.dumps(observed))
        stderr.write_bytes(b"")
        return dict(argv=argv, cwd=str(cwd), environment=env, returncode=0,
                    stdout=RUN.BASE.artifact(stdout), stderr=RUN.BASE.artifact(stderr))

    def test_full_three_state_protocol_archives_before_delete_and_keeps_proofs_unshared(self):
        args = self.protocol_arguments()
        manifest = json.loads((RUN.ROOT / "manifest.json").read_bytes())
        record = dict(runs=[], same_root_states=[])
        self.operations, self.imported = [], []
        destroy = RUN.destroy_state

        def inspect_then_destroy(state):
            self.assertTrue(Path(state["execution_fragment"]["path"]).is_file())
            destroy(state)

        with patch.object(RUN.BASE.BIND, "verify", return_value=(manifest, "synthetic")), \
             patch.object(RUN.BASE, "tool_observations"), \
             patch.object(RUN.BASE, "execute", side_effect=self.protocol_execute), \
             patch.object(RUN, "destroy_state", side_effect=inspect_then_destroy):
            RUN.run_all(args, record)
        self.assertEqual(record["status"], "observed")
        self.assertEqual(self.operations, ["comparison-state", "export", "import", "export", "import", "export"])
        self.assertEqual(self.imported, [b"native export 1", b"native export 2"])
        self.assertEqual(len(record["runs"]), 3)
        self.assertEqual(sum(len(run["commands"]) for run in record["runs"]), 6)
        self.assertEqual(len({run["cwd"] for run in record["runs"]}), 1)
        self.assertFalse(args.active_root.exists())
        for run in record["runs"]:
            for command in run["commands"]:
                self.assertTrue(Path(command["stdout"]["retained"]["path"]).is_file())
                self.assertEqual(len(command["admission_artifacts"]), 1)
        self.assertTrue(all(state["destroyed"] for state in record["same_root_states"]))

    def test_retention_failure_preserves_original_state(self):
        state = RUN.acquire_root(self.active, 1)
        (self.active / "evidence").write_bytes(b"keep me")
        with patch.object(RUN.shutil, "copytree", side_effect=OSError("disk full")):
            with self.assertRaisesRegex(OSError, "disk full"):
                RUN.archive_state(state, self.root / "retained")
        self.assertEqual((self.active / "evidence").read_bytes(), b"keep me")
        self.assertNotIn("retained_ns", state)
        self.assertFalse(state["destroyed"])


if __name__ == "__main__":
    unittest.main()
