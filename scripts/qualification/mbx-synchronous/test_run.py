"""Harness regressions use no Cargo/compiler workload and confer no authority."""

import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("fixture_run", Path(__file__).with_name("run.py"))
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)


class HarnessTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()

    def test_subprocess_receives_exact_environment_and_no_timeout(self):
        completed = argparse.Namespace(returncode=7)
        env = {"ONLY_REVIEWED": "value"}
        with patch.object(RUN.subprocess, "run", return_value=completed) as launch:
            result = RUN.execute(["/owned/tool", "build"], self.root, env, self.root, "task")
        self.assertEqual(result["returncode"], 7)
        self.assertEqual(result["environment"], env)
        self.assertEqual(launch.call_args.kwargs["env"], env)
        self.assertNotIn("timeout", launch.call_args.kwargs)
        self.assertEqual(result["stdout"]["sha256"], RUN.digest(self.root / "task.stdout"))

    def test_unreviewed_ancestor_config_rejected(self):
        workspace = self.root / "workspace"
        workspace.mkdir()
        (self.root / ".cargo").mkdir()
        (self.root / ".cargo/config.toml").write_text("[source.crates-io]\nreplace-with='bad'\n")
        env = {"HOME": str(self.root / "home"), "CARGO_HOME": str(self.root / "cargo")}
        with self.assertRaisesRegex(ValueError, "configuration discovery"):
            RUN.config_inputs(workspace, env)

    def test_unknown_source_requires_explicit_observation_mode(self):
        args = argparse.Namespace(mbx_source_receipt=None, mbx_source_receipt_sha256=None,
                                  compiler_source_receipt=None,
                                  compiler_source_receipt_sha256=None, prequalified_local=False)
        with self.assertRaisesRegex(ValueError, "source receipt required"):
            RUN.source_receipts(args)
        args.prequalified_local = True
        self.assertEqual(RUN.source_receipts(args), {"mbx": None, "compiler": None})

    def test_source_receipt_digest_is_independent_required_input(self):
        receipt = self.root / "receipt.json"
        receipt.write_text(json.dumps({"qualified": True}))
        args = argparse.Namespace(mbx_source_receipt=receipt, mbx_source_receipt_sha256="wrong",
                                  compiler_source_receipt=None,
                                  compiler_source_receipt_sha256=None, prequalified_local=True)
        with self.assertRaisesRegex(ValueError, "receipt digest differs"):
            RUN.source_receipts(args)

    def test_export_is_owned_directory_transport(self):
        bundle = self.root / "bundle"
        bundle.mkdir()
        (bundle / "receipt").write_text("evidence")
        (self.root / "export.json").write_text('{"exported": true}')
        workspace = self.root / "workspace"
        workspace.mkdir()
        run = dict(cwd=str(workspace), environment={"MBX_CACHE_DIR": "/owned/cache"}, transport=[])
        args = argparse.Namespace(mbx=Path("/owned/mbx"), group="group", bundle_witnesses={})
        with patch.object(RUN, "execute", return_value={"returncode": 0, "stdout": {"path": str(self.root / "export.json")}}) as launch:
            RUN.transport(args, run, "export", bundle, self.root / "baseline", {})
        self.assertEqual(launch.call_args.args[0], [Path("/owned/mbx"), "cache", "export",
                         bundle, "--format", "directory", "--json", "--group", "group",
                         "--compare", self.root / "baseline"])

    def test_import_consumes_copy_and_retains_original(self):
        workspace = self.root / "workspace"
        workspace.mkdir()
        bundle = self.root / "retained"
        bundle.mkdir()
        (bundle / "object").write_bytes(b"owned object")
        args_witness = RUN.write_json(self.root / "retained-inventory.json", RUN.tree(bundle))
        baseline = self.root / "baseline.json"
        baseline.write_text("{}")
        run = dict(cwd=str(workspace), environment={}, transport=[])
        args = argparse.Namespace(mbx=Path("/owned/mbx"), group="group", bundle_witnesses={})

        args.bundle_witnesses[str(bundle)] = args_witness

        def consume(argv, *unused):
            RUN.shutil.rmtree(argv[3])
            return {"returncode": 0}

        with patch.object(RUN, "execute", side_effect=consume):
            result = RUN.transport(args, run, "import", bundle, baseline,
                                   {"commands": [["cargo", "build", "--lib"]]})
        self.assertEqual(result, bundle)
        self.assertEqual((bundle / "object").read_bytes(), b"owned object")
        self.assertFalse(run["transport"][0]["consumed_copy_exists_after"])

    def test_unchanged_export_reuses_retained_bundle_for_third_run(self):
        retained = self.root / "retained"
        retained.mkdir()
        (retained / "object").write_bytes(b"observed native bundle")
        args = argparse.Namespace(mbx=Path("/owned/mbx"), group="group", bundle_witnesses={})
        report = self.root / "skipped-export.json"
        report.write_text('{"exported": false}')
        args.bundle_witnesses[str(retained)] = RUN.write_json(self.root / "retained-inventory.json",
                                                             RUN.tree(retained))
        imported = []

        def native(argv, *unused):
            if argv[2] == "import":
                imported.append((argv[3] / "object").read_bytes())
                RUN.shutil.rmtree(argv[3])
            return {"returncode": 0, "stdout": {"path": str(report)}}

        with patch.object(RUN, "execute", side_effect=native):
            for number in (2, 3):
                root = self.root / str(number)
                workspace = root / "workspace"
                workspace.mkdir(parents=True)
                baseline = root / "baseline"
                baseline.write_text("{}")
                run = dict(cwd=str(workspace), environment={}, transport=[])
                RUN.transport(args, run, "import", retained, baseline,
                              {"commands": [["cargo", "build", "--lib"]]})
                exported = RUN.transport(args, run, "export", root / "candidate", baseline, {})
                retained = exported or retained
        self.assertEqual(imported, [b"observed native bundle"] * 2)
        self.assertTrue((retained / "object").is_file())

    def test_native_shaped_admission_ledger_retained_as_opaque(self):
        reports = self.root / "reports"
        ledger = reports / ".mbx-admissions-session"
        ledger.mkdir(parents=True)
        (reports / "session.json").write_text('{"completed": true}')
        for name in ("identity", ".lock", "event.accepted", "event.terminal", "closed"):
            (ledger / name).write_bytes(b"opaque; not public report JSON")
        record = {}
        RUN.retain_reports(record, reports)
        self.assertEqual(len(record["report_artifacts"]), 1)
        self.assertEqual(len(record["admission_artifacts"]), 5)
        self.assertEqual(Path(record["report_artifacts"][0]["path"]).parent, reports)
        self.assertTrue(all(Path(item["path"]).parent == ledger
                            for item in record["admission_artifacts"]))

    def seed_arguments(self):
        seed = self.root / "seed"
        archive = Path("registry/cache/registry-id/itoa.crate")
        source = Path("registry/src/registry-id/itoa")
        for relative, data in ((archive, b"archive"), (source / "src/lib.rs", b"source"),
                               (Path("registry/index/registry-id/config.json"), b"{}"),
                               (Path("registry/index/registry-id/.cache/it/oa/itoa"), b"index")):
            path = seed / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        return argparse.Namespace(registry_home=seed, archive_relative=archive,
                                  source_relative=source)

    def test_cargo_cache_tag_is_seeded_and_remains_strictly_bound(self):
        args = self.seed_arguments()
        destination = self.root / "cargo-home"
        RUN.seed_registry(args, destination)
        tag = destination / "registry/CACHEDIR.TAG"
        self.assertEqual(RUN.digest(tag),
                         "6d9d1d216e0f83abc5e5662ca62c92b4f23009466b54fa27321a69acdb778bb2")
        before = RUN.tree(destination / "registry")
        # Cargo's genuine observed write adds no new inventory once seeded.
        tag.write_bytes(RUN.CARGO_CACHE_TAG)
        self.assertEqual(RUN.tree(destination / "registry"), before)
        tag.write_bytes(b"changed metadata")
        self.assertNotEqual(RUN.tree(destination / "registry"), before)

    def test_seed_keeps_source_index_and_archive_mutations_rejected(self):
        args = self.seed_arguments()
        workspace = self.root / "workspace"
        workspace.mkdir()
        (self.root / "logs").mkdir()
        destination = self.root / "cargo-home"
        RUN.seed_registry(args, destination)
        witness = RUN.write_json(self.root / "registry-inputs.json", RUN.tree(destination / "registry"))
        run = dict(cwd=str(workspace), registry_inputs=witness,
                   registry_archive=str(destination / args.archive_relative),
                   registry_source=str(destination / args.source_relative))
        bind_args = argparse.Namespace(expected_manifest_sha256="reviewed")
        paths = (args.archive_relative, args.source_relative / "src/lib.rs",
                 Path("registry/index/registry-id/.cache/it/oa/itoa"))
        for relative in paths:
            with self.subTest(input=str(relative)):
                target = destination / relative
                original = target.read_bytes()
                target.write_bytes(b"mutated")
                with patch.object(RUN.BIND, "verify", return_value=({}, "reviewed")):
                    with self.assertRaisesRegex(ValueError, "copied registry inputs changed"):
                        RUN.bind(bind_args, run, "after")
                target.write_bytes(original)

    def test_source_inventory_rejects_symlink(self):
        (self.root / "source").write_text("source")
        (self.root / "alias").symlink_to(self.root / "source")
        with self.assertRaisesRegex(ValueError, "symlink forbidden"):
            RUN.tree(self.root)


if __name__ == "__main__":
    unittest.main()
