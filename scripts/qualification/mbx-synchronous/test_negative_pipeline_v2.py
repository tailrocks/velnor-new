"""Full physical harness protocol simulation; never launches compiler/native tools."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

RUN = load("negative_pipeline", "run_negative_v2.py")
HELPERS = load("negative_physical_helpers", "test_run_same_root.py")
V2TEST = load("negative_transport_helpers", "test_run_v2.py")


def _cargo_target(name, source):
    return dict(name=name, kind=["lib"], crate_types=["lib"],
                src_path=str(source), edition="2024")


def _cargo_metadata(workspace, env):
    root_id = "path+" + str(workspace) + "#mbx-synchronous-registry-fixture@0.0.0"
    itoa_id = "registry+https://github.com/rust-lang/crates.io-index#itoa@1.0.18"
    root_manifest = workspace / "Cargo.toml"
    itoa_manifest = Path(env["CARGO_HOME"]) / "registry/src/id/itoa/Cargo.toml"
    root_target = _cargo_target("mbx_synchronous_registry_fixture", workspace / "src/lib.rs")
    itoa_target = _cargo_target("itoa", itoa_manifest.parent / "src/lib.rs")
    return dict(version=1,
        packages=[
            dict(id=root_id, name="mbx-synchronous-registry-fixture", version="0.0.0",
                 manifest_path=str(root_manifest), targets=[root_target]),
            dict(id=itoa_id, name="itoa", version="1.0.18", source=
                 "registry+https://github.com/rust-lang/crates.io-index",
                 manifest_path=str(itoa_manifest), targets=[itoa_target])],
        workspace_members=[root_id],
        resolve=dict(nodes=[
            dict(id=root_id, deps=[dict(name="itoa", pkg=itoa_id,
                                        dep_kinds=[dict(kind=None, target=None)])]),
            dict(id=itoa_id, deps=[])]))


def _make_executor(sim, args, failure, observer_failure, libraries, observer_calls):
    def execute(argv, cwd, env, logs, name):
        argv = [str(v) for v in argv]
        if name in ("observer-compile", "observer-run"):
            observer_calls.append(name)
            logs.mkdir(exist_ok=True)
            stdout, stderr = logs / (name+".stdout"), logs / (name+".stderr")
            if name == "observer-compile":
                Path(argv[-1]).write_bytes(b"mock newly linked binary")
                stdout.write_bytes(b"")
            else:
                stdout.write_text("9\n" if len(libraries)==1 else "10\n")
            stderr.write_bytes(b"")
            if observer_failure:
                (args.sdk_root / "context").write_bytes(b"mutated SDK")
                (args.output / "origin-inputs.json").write_bytes(b"invalid JSON")
            return dict(argv=argv, cwd=str(cwd), environment=env,
                returncode=int(observer_failure), stdout=RUN.BASE.artifact(stdout),
                stderr=RUN.BASE.artifact(stderr))
        if name == "cargo-resolution":
            logs.mkdir(exist_ok=True)
            stdout, stderr = logs / "cargo-resolution.stdout", logs / "cargo-resolution.stderr"
            stdout.write_text(json.dumps(_cargo_metadata(Path(cwd), env)))
            stderr.write_bytes(b"")
            return dict(argv=argv, cwd=str(cwd), environment=env, returncode=0,
                stdout=RUN.BASE.artifact(stdout), stderr=RUN.BASE.artifact(stderr))
        result = sim.protocol_execute(argv, cwd, env, logs, name)
        operation = RUN.V2.owned_transport(argv)
        if operation == "export":
            public = V2TEST.export_report(True)
        elif operation == "import":
            public = dict(version=1, actions=1, objects=1, bytes=8,
                comparison_state_recorded=True, workspace_restored=False,
                workspace_restore="skipped_incompatible")
        elif operation == "comparison-state":
            public = dict(version=1, valid=True, empty=True)
        else:
            libraries.append(result)
            metadata = _cargo_metadata(Path(cwd), env)
            root_package, itoa_package = metadata["packages"]
            target_dir = Path(env["CARGO_TARGET_DIR"]) / "debug/deps"
            target_dir.mkdir(parents=True, exist_ok=True)
            if failure and len(libraries) == 2:
                public = dict(reason="build-finished", success=False)
            else:
                records = []
                for package, target_value, filename, payload in (
                    (root_package, root_package["targets"][0],
                     "libmbx_synchronous_registry_fixture-0123456789abcdef.rlib",
                     b"mock current root rlib"),
                    (itoa_package, itoa_package["targets"][0],
                     "libitoa-0123456789abcdef.rlib", b"mock current itoa rlib")):
                    artifact = target_dir / filename
                    artifact.write_bytes(payload)
                    rmeta = artifact.with_suffix(".rmeta")
                    rmeta.write_bytes(payload + b" metadata")
                    records.append(dict(reason="compiler-artifact", package_id=package["id"],
                        manifest_path=package["manifest_path"], target=target_value,
                        filenames=[str(artifact), str(rmeta)]))
                public = records + [dict(reason="build-finished", success=True)]
            if failure and len(libraries)==2:
                result["returncode"] = 1
                Path(result["stderr"]["path"]).write_text("T06_EXPECTED_FAILURE")
                result["stderr"] = RUN.BASE.artifact(Path(result["stderr"]["path"]))
        output = public if type(public) is list else [public]
        Path(result["stdout"]["path"]).write_text(
            "".join(json.dumps(item)+"\n" for item in output))
        result["stdout"] = RUN.BASE.artifact(Path(result["stdout"]["path"]))
        return result
    return execute


class PipelineTests(unittest.TestCase):
    def test_standalone_workspace_is_materialized_exactly_and_privately(self):
        sim = HELPERS.SameRootTests(methodName="test_retention_failure_preserves_original_state")
        sim.setUp()
        self.addCleanup(sim.doCleanups)
        manifest = json.loads((ROOT / "manifest.json").read_bytes())
        private = sim.root / "private-tmp"
        private.mkdir(mode=0o700)
        workspace = private / "workspace"
        RUN.BASE.BIND.materialize_fixture(workspace, manifest, private_root=private)
        self.assertEqual(RUN.BASE.BIND.inventory(workspace), manifest["fixture"]["files"])
        self.assertEqual(workspace.stat().st_mode & 0o777, 0o700)
        self.assertTrue(all(path.stat().st_mode & 0o777 == 0o600
                            for path in workspace.rglob("*") if path.is_file()))
        self.assertFalse((ROOT / "registry-fixture").exists())

    def setup_pipeline(self, failure):
        sim = HELPERS.SameRootTests(methodName="test_retention_failure_preserves_original_state")
        sim.setUp()
        self.addCleanup(sim.doCleanups)
        args = sim.protocol_arguments()
        args.sdk_root = sim.root / "sdk"
        args.sdk_root.mkdir()
        (args.sdk_root / "context").write_bytes(b"SDK mock context")
        args.linker = None
        sdk = RUN.INPUTS.context_inventory(args.sdk_root)
        args.expected_sdk_inventory_sha256 = RUN.BASE.write_json(sim.root / "sdk.json", sdk)["sha256"]
        baseline = sim.root / "baseline"
        positive_manifest = json.loads((ROOT / "manifest.json").read_bytes())
        RUN.BASE.BIND.materialize_fixture(baseline, positive_manifest)
        successor = sim.root / "successor"
        shutil.copytree(baseline, successor)
        (successor / "src/lib.rs").write_text("compile_error!(\"T06_EXPECTED_FAILURE\");" if failure else "pub fn observed() -> usize { 10 }")
        def spec(root):
            files = RUN.BASE.BIND.inventory(root)
            return dict(root=str(root), files=files, inventory_sha256=RUN.BASE.BIND.inventory_sha(files))
        observer = sim.root / "observer.rs"
        observer.write_bytes(b"synthetic observer source")
        argv = positive_manifest["commands"][0]
        case = dict(id="T06-compile-error" if failure else "T06-B", baseline="a", successor="b", argv=argv,
                    baseline_stdout="9\n", successor_stdout="10\n")
        if failure:
            case.update(expected_build_exit="nonzero", expected_diagnostic="T06_EXPECTED_FAILURE")
        blueprint = dict(schema=1, fixtures={"a":spec(baseline), "b":spec(successor)}, cases=[case],
            frozen_mbx={"binary_sha256":args.mbx_sha256}, observer=dict(RUN.BASE.artifact(observer), baseline_stdout="9\n"))
        args.blueprint = sim.root / "blueprint.json"
        args.expected_blueprint_sha256 = RUN.BASE.write_json(args.blueprint, blueprint)["sha256"]
        args.case = case["id"]
        return sim, args, blueprint

    def _assert_pipeline_result(self, sim, args, failure, observer_failure,
                                observer_calls, record):
        self.assertFalse(args.active_root.exists())
        self.assertTrue(all(state["destroyed"] for state in record["states"]))
        for run in record["runs"]:
            command = run["commands"][0]
            self.assertTrue(Path(command["stdout"]["retained"]["path"]).is_file())
            self.assertTrue(command["report_artifacts"])
        if observer_failure:
            self.assertEqual(next(g for g in record["after_guards"] if g["name"] == "sdk")["status"], "changed")
            self.assertIn("primary_error", record)
            self.assertEqual(next(g for g in record["after_guards"] if g["name"] == "origin-receipt")["status"], "changed")
            self.assertEqual(next(g for g in record["after_guards"] if g["name"] == "tool-rustc")["status"], "unchanged")
            return
        self.assertEqual(record["status"], "observed-local-negative-case")
        self.assertEqual(sim.imported, [b"native export 1"])
        self.assertEqual(sim.operations, ["comparison-state", "export", "import"] +
                         ([] if failure else ["export"]))
        self.assertEqual(observer_calls, ["observer-compile", "observer-run"] *
                         (1 if failure else 2))
        if failure:
            failed = record["runs"][1]
            self.assertIsNone(failed["observer"])
            self.assertEqual(failed["export_skipped_reason"], "failed-current-library-command")
            stderr = Path(failed["commands"][0]["stderr"]["retained"]["path"])
            self.assertIn("T06_EXPECTED_FAILURE", stderr.read_text())

    def pipeline(self, failure=False, observer_failure=False):
        sim, args, _ = self.setup_pipeline(failure)
        sim.operations, sim.imported = [], []
        libraries, observer_calls = [], []
        execute = _make_executor(sim, args, failure, observer_failure, libraries, observer_calls)
        record = dict(runs=[], states=[], native_authority=None)
        with patch.object(RUN.BASE.BIND, "verify"), \
             patch.object(RUN.BASE.BIND, "verify_registry"), \
             patch.object(RUN.BASE, "tool_observations"), \
             patch.object(RUN.BASE, "execute", side_effect=execute):
            with RUN.V2.strict_execution(RUN.BASE, RUN.PROTOCOL):
                if observer_failure:
                    with self.assertRaisesRegex(ValueError, "observer compilation failed"):
                        RUN.run_all(args, record)
                else:
                    RUN.run_all(args, record)
        self._assert_pipeline_result(sim, args, failure, observer_failure,
                                     observer_calls, record)

    def test_two_states_public_rlib_observer_and_immutable_import(self):
        self.pipeline()

    def test_compile_failure_retains_reports_skips_observer_export(self):
        self.pipeline(failure=True)

    def test_observer_failure_still_archives_and_records_sdk_mutation(self):
        self.pipeline(observer_failure=True)
