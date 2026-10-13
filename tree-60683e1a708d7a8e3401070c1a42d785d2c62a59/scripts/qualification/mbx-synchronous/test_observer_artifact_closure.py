"""Focused synthetic regressions; no Cargo or Rust compiler is invoked."""

import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUN = load("observer_closure_driver", "run_negative_v2.py")
INPUTS = RUN.INPUTS
BASE = RUN.BASE


class ArtifactClosureTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.run_root = self.root / "run"
        self.workspace = self.run_root / "workspace"
        self.target_root = self.run_root / "target"
        self.deps = self.target_root / "debug" / "deps"
        self.logs = self.run_root / "logs"
        self.workspace.mkdir(parents=True)
        self.deps.mkdir(parents=True)
        self.logs.mkdir()
        (self.workspace / "Cargo.toml").write_text("[package]\nname='fixture'\n")
        (self.workspace / "src").mkdir()
        (self.workspace / "src/lib.rs").write_text("pub fn value() {}\n")

        self.root_id = "path+" + str(self.workspace) + "#mbx-synchronous-registry-fixture@0.0.0"
        self.dep_id = "registry+https://github.com/rust-lang/crates.io-index#itoa@1.0.18"
        self.root_target = self.target("mbx_synchronous_registry_fixture",
                                       self.workspace / "src/lib.rs")
        self.dep_source = self.root / "registry/itoa-1.0.18/src/lib.rs"
        self.dep_source.parent.mkdir(parents=True)
        self.dep_source.write_text("pub fn stub() {}\n")
        self.dep_manifest = self.dep_source.parents[1] / "Cargo.toml"
        self.dep_manifest.write_text("[package]\nname='itoa'\n")
        self.dep_target = self.target("itoa", self.dep_source)
        self.root_rlib = self.deps / "libmbx_synchronous_registry_fixture.rlib"
        self.dep_rlib = self.deps / "libitoa-9e519a847d238e22.rlib"
        self.root_rlib.write_bytes(b"synthetic root rlib bytes")
        self.dep_rlib.write_bytes(b"synthetic itoa rlib bytes")
        (self.deps / "libitoa-stale.rlib").write_bytes(b"stale target artifact")
        self.metadata_path = self.logs / "cargo-resolution.stdout"
        self.stdout = self.logs / "library.stdout"
        self.metadata = self.metadata_value()
        self.write_metadata()
        self.records = [self.artifact_message(self.root_id, self.workspace / "Cargo.toml",
                                               self.root_target, self.root_rlib),
                        self.artifact_message(self.dep_id, self.dep_manifest,
                                               self.dep_target, self.dep_rlib)]
        self.write_build()

    @staticmethod
    def target(name, source):
        return dict(name=name, kind=["lib"], crate_types=["lib"], src_path=str(source),
                    edition="2024")

    def metadata_value(self):
        root_package = dict(id=self.root_id, name="mbx-synchronous-registry-fixture",
                            version="0.0.0", manifest_path=str(self.workspace / "Cargo.toml"),
                            targets=[self.root_target])
        dep_package = dict(id=self.dep_id, name="itoa", version="1.0.18",
                           manifest_path=str(self.dep_manifest), source="registry+https://github.com/rust-lang/crates.io-index",
                           targets=[self.dep_target])
        return dict(version=1, packages=[root_package, dep_package],
                    workspace_members=[self.root_id],
                    resolve=dict(nodes=[dict(id=self.root_id,
                                             deps=[dict(name="itoa", pkg=self.dep_id,
                                                        dep_kinds=[dict(kind=None, target=None)])]),
                                        dict(id=self.dep_id, deps=[])]))

    def artifact_message(self, package_id, manifest, target, rlib):
        return dict(reason="compiler-artifact", package_id=package_id,
                    manifest_path=str(manifest), target=target,
                    filenames=[str(rlib), str(rlib.with_suffix(".rmeta"))])

    def write_metadata(self):
        self.metadata_path.write_text(json.dumps(self.metadata, sort_keys=True))
        self.metadata_receipt = BASE.artifact(self.metadata_path)

    def write_build(self):
        lines = [json.dumps(value) for value in self.records]
        lines.append(json.dumps(dict(reason="build-finished", success=True)))
        self.stdout.write_text("\n".join(lines) + "\n")
        self.build_receipt = BASE.artifact(self.stdout)

    def closure(self, build_receipt=None):
        return INPUTS.public_rlib(self.stdout, self.workspace, self.target_root,
                                  self.metadata_path, self.metadata_receipt,
                                  build_receipt or self.build_receipt)

    def test_exact_fresh_closure_stages_transitive_rlib_and_excludes_stale_file(self):
        closure = self.closure()
        self.assertEqual(Path(closure["artifact"]["path"]), self.root_rlib)
        self.assertEqual(closure["artifact"], BASE.artifact(self.root_rlib))
        self.assertEqual([owner["package_name"] for owner in closure["public_artifact_owners"]],
                         ["mbx-synchronous-registry-fixture", "itoa"])
        staging = INPUTS.stage_artifacts(closure, self.run_root / "observer-inputs")
        staged_names = {Path(item["path"]).name for item in staging["inventory"]}
        self.assertEqual(staged_names, {self.root_rlib.name, self.dep_rlib.name})
        self.assertNotIn("libitoa-stale.rlib", staged_names)
        self.assertTrue(all(item["mode"] == "0o600" for item in staging["inventory"]))
        self.assertEqual(staging["directory"], str(self.run_root / "observer-inputs"))

    def test_resolution_command_rejects_feature_or_target_scope_it_cannot_mirror(self):
        cargo = self.root / "toolchain/bin/cargo"
        supported = ["cargo", "build", "--locked", "--offline", "--lib",
                     "--message-format=json-render-diagnostics"]
        argv = INPUTS.resolution_argv(cargo, self.workspace, supported)
        self.assertEqual(argv[1:3], ["metadata", "--locked"])
        with self.assertRaisesRegex(ValueError, "unsupported package, feature, target"):
            INPUTS.resolution_argv(cargo, self.workspace,
                                   supported + ["--features", "extra"])
        with self.assertRaisesRegex(ValueError, "Cargo build command required"):
            INPUTS.resolution_argv(cargo, self.workspace,
                                   ["cargo", "check", *supported[2:]])

    def test_missing_transitive_or_duplicate_current_artifact_rejects(self):
        original = copy.deepcopy(self.records)
        self.records = [original[0]]
        self.write_build()
        with self.assertRaisesRegex(ValueError, "normal dependency rlib is missing"):
            self.closure()
        self.records = original + [copy.deepcopy(original[1])]
        self.write_build()
        with self.assertRaisesRegex(ValueError, "duplicate current Cargo compiler-artifact"):
            self.closure()
        self.stdout.write_text("\n".join([json.dumps(item) for item in original] +
                                          [json.dumps(dict(reason="build-finished", success=True)),
                                           json.dumps(original[0])]) + "\n")
        self.build_receipt = BASE.artifact(self.stdout)
        with self.assertRaisesRegex(ValueError, "messages after build-finished"):
            self.closure()

    def test_replaced_build_log_does_not_match_actual_cargo_command_receipt(self):
        command_receipt = self.build_receipt
        self.stdout.write_text(json.dumps(dict(reason="build-finished", success=True)) + "\n")
        with self.assertRaisesRegex(ValueError, "differs from command receipt"):
            self.closure(command_receipt)

    def test_foreign_symlink_unbound_and_target_specific_inputs_reject(self):
        original_records = copy.deepcopy(self.records)
        original_metadata = copy.deepcopy(self.metadata)

        foreign = self.root / "foreign.rlib"
        foreign.write_bytes(b"foreign")
        self.records = copy.deepcopy(original_records)
        self.records[1]["filenames"][0] = str(foreign)
        self.write_build()
        with self.assertRaisesRegex(ValueError, "escapes current owned target"):
            self.closure()

        link = self.deps / "libitoa-link.rlib"
        link.symlink_to(self.dep_rlib)
        self.records = copy.deepcopy(original_records)
        self.records[1]["filenames"][0] = str(link)
        self.write_build()
        with self.assertRaises((ValueError, OSError)):
            self.closure()
        link.unlink()

        self.records = copy.deepcopy(original_records)
        self.records[1]["package_id"] = "unbound-package-id"
        self.write_build()
        with self.assertRaisesRegex(ValueError, "unbound to current metadata package"):
            self.closure()

        self.records = original_records
        self.metadata = original_metadata
        self.metadata["resolve"]["nodes"][0]["deps"][0]["dep_kinds"][0]["target"] = "cfg(unix)"
        self.write_metadata()
        self.write_build()
        with self.assertRaisesRegex(ValueError, "target-specific normal dependency"):
            self.closure()

    def test_source_tamper_and_foreign_staged_membership_reject(self):
        closure = self.closure()
        staging = INPUTS.stage_artifacts(closure, self.run_root / "observer-inputs")
        expected = {Path(item["path"]).name: item for item in staging["source_artifacts"]}
        (Path(staging["directory"]) / "libforeign.rlib").write_bytes(b"foreign")
        with self.assertRaisesRegex(ValueError, "staged dependency membership differs"):
            INPUTS.staged_inventory(Path(staging["directory"]), expected)
        (Path(staging["directory"]) / "libforeign.rlib").unlink()
        staged_dep = Path(staging["directory"]) / self.dep_rlib.name
        staged_dep.write_bytes(b"staged bytes changed")
        with self.assertRaisesRegex(ValueError, "staged artifact differs"):
            INPUTS.staged_inventory(Path(staging["directory"]), expected)
        self.dep_rlib.write_bytes(b"changed after current Cargo output")
        with self.assertRaisesRegex(ValueError, "Cargo dependency closure changed"):
            INPUTS.validate_source_closure(closure)

    def test_observer_uses_staged_extern_and_search_path(self):
        closure = self.closure()
        observer_source = self.root / "observer.rs"
        observer_source.write_text("fn main() {}\n")
        logs = self.root / "observer-logs"
        logs.mkdir()
        binary = self.run_root / "observer-binary"
        run = dict(cwd=str(self.workspace),
                   environment={"CARGO_TARGET_DIR": str(self.target_root), "PATH": "/bin"},
                   resolution={"metadata": self.metadata_receipt})
        args = type("Args", (), dict(rustc=self.root / "toolchain/bin/rustc",
                                     sdk_root=self.root / "sdk"))()
        blueprint = dict(observer=BASE.artifact(observer_source))
        command = dict(returncode=0, stdout=closure["build_stdout"])
        calls = []

        def measured(argv, cwd, environment, unused_logs, name):
            values = [str(value) for value in argv]
            calls.append(values)
            stdout, stderr = logs / (name + ".stdout"), logs / (name + ".stderr")
            stdout.write_text("9\n" if name == "observer-run" else "")
            stderr.write_bytes(b"")
            if name == "observer-compile":
                self.assertIn("dependency=" + str(self.run_root / "observer-inputs"), values)
                self.assertIn("mbx_synchronous_registry_fixture=" +
                              str(self.run_root / "observer-inputs" / self.root_rlib.name), values)
                self.assertNotIn("dependency=" + str(self.deps), values)
                Path(values[-1]).write_bytes(b"synthetic observer executable")
            return dict(argv=values, returncode=0, stdout=BASE.artifact(stdout),
                        stderr=BASE.artifact(stderr))

        with patch.object(RUN, "measured", side_effect=measured):
            RUN.observer(args, run, blueprint, command, "9\n")
        self.assertEqual([item[0] for item in calls], [str(args.rustc), str(binary)])
        self.assertEqual(run["observer"]["dependency_staging"]["directory"],
                         str(self.run_root / "observer-inputs"))
        self.assertIsNone(run["observer"]["native_authority"])

    def test_observer_rejects_owned_dependency_changed_during_compilation(self):
        closure = self.closure()
        observer_source = self.root / "observer.rs"
        observer_source.write_text("fn main() {}\n")
        logs = self.root / "observer-logs"
        logs.mkdir()
        binary = self.run_root / "observer-binary"
        run = dict(cwd=str(self.workspace),
                   environment={"CARGO_TARGET_DIR": str(self.target_root), "PATH": "/bin"},
                   resolution={"metadata": self.metadata_receipt})
        args = type("Args", (), dict(rustc=self.root / "toolchain/bin/rustc",
                                     sdk_root=self.root / "sdk"))()
        blueprint = dict(observer=BASE.artifact(observer_source))
        command = dict(returncode=0, stdout=closure["build_stdout"])

        def measured(argv, cwd, environment, unused_logs, name):
            values = [str(value) for value in argv]
            stdout, stderr = logs / (name + ".stdout"), logs / (name + ".stderr")
            stdout.write_text("")
            stderr.write_bytes(b"")
            if name == "observer-compile":
                self.dep_rlib.write_bytes(b"changed while pinned rustc compiled")
                Path(values[-1]).write_bytes(b"synthetic observer executable")
            return dict(argv=values, returncode=0, stdout=BASE.artifact(stdout),
                        stderr=BASE.artifact(stderr))

        with patch.object(RUN, "measured", side_effect=measured):
            with self.assertRaisesRegex(ValueError, "Cargo dependency closure changed"):
                RUN.observer(args, run, blueprint, command, "9\n")

    def test_observer_rejects_staged_dependency_changed_during_compilation(self):
        closure = self.closure()
        observer_source = self.root / "observer.rs"
        observer_source.write_text("fn main() {}\n")
        logs = self.root / "observer-logs"
        logs.mkdir()
        binary = self.run_root / "observer-binary"
        run = dict(cwd=str(self.workspace),
                   environment={"CARGO_TARGET_DIR": str(self.target_root), "PATH": "/bin"},
                   resolution={"metadata": self.metadata_receipt})
        args = type("Args", (), dict(rustc=self.root / "toolchain/bin/rustc",
                                     sdk_root=self.root / "sdk"))()
        blueprint = dict(observer=BASE.artifact(observer_source))
        command = dict(returncode=0, stdout=closure["build_stdout"])

        def measured(argv, cwd, environment, unused_logs, name):
            values = [str(value) for value in argv]
            stdout, stderr = logs / (name + ".stdout"), logs / (name + ".stderr")
            stdout.write_text("")
            stderr.write_bytes(b"")
            if name == "observer-compile":
                dependency = next(value.split("=", 1)[1] for value in values
                                  if value.startswith("dependency="))
                (Path(dependency) / self.dep_rlib.name).write_bytes(
                    b"staged dependency changed while pinned rustc compiled")
                Path(values[-1]).write_bytes(b"synthetic observer executable")
            return dict(argv=values, returncode=0, stdout=BASE.artifact(stdout),
                        stderr=BASE.artifact(stderr))

        with patch.object(RUN, "measured", side_effect=measured):
            with self.assertRaisesRegex(ValueError, "staged artifact differs"):
                RUN.observer(args, run, blueprint, command, "9\n")


if __name__ == "__main__":
    unittest.main()
