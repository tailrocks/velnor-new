"""Negative fixture/observer regressions use synthetic bytes; no actual compiler execution."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("negative_runtime_test", ROOT / "run_negative_v2.py")
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)
INPUTS = RUN.INPUTS


class NegativeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.workspace = self.root / "workspace"
        (self.workspace / "src").mkdir(parents=True)
        (self.workspace / "Cargo.toml").write_text('[package]\nname="fixture"\nversion="0.0.0"\n')
        (self.workspace / "src/lib.rs").write_text("pub fn value() -> usize { 9 }\n")

    def specification(self):
        records = INPUTS.BASE.BIND.inventory(self.workspace)
        return dict(root=str(self.workspace), files=records,
                    inventory_sha256=INPUTS.BASE.BIND.inventory_sha(records))

    def public_message(self, artifact):
        return dict(reason="compiler-artifact", package_id="path+file://" + str(self.workspace) + "#fixture@0.0.0",
                    manifest_path=str(self.workspace / "Cargo.toml"),
                    target=dict(name="fixture", kind=["lib"], crate_types=["lib"],
                                src_path=str(self.workspace / "src/lib.rs")), filenames=[str(artifact)])

    def test_public_artifact_selection_uses_actual_report_path(self):
        artifact = self.root / "actual-emitted-library.rlib"
        artifact.write_bytes(b"synthetic library")
        stdout = self.root / "cargo.stdout"
        stdout.write_text(json.dumps(self.public_message(artifact)) + "\n")
        selected = INPUTS.public_rlib(stdout, self.workspace, self.root)
        self.assertEqual(selected["artifact"]["path"], str(artifact))
        self.assertEqual(selected["artifact"]["sha256"], INPUTS.BASE.digest(artifact))

    def test_duplicate_or_external_public_rlib_is_rejected(self):
        artifact = self.root / "observed.rlib"
        artifact.write_bytes(b"synthetic")
        stdout = self.root / "cargo.stdout"
        message = json.dumps(self.public_message(artifact)) + "\n"
        stdout.write_text(message * 2)
        with self.assertRaisesRegex(ValueError, "exactly one"):
            INPUTS.public_rlib(stdout, self.workspace, self.root)
        stdout.write_text(message)
        with self.assertRaisesRegex(ValueError, "escapes owned"):
            INPUTS.public_rlib(stdout, self.workspace, self.workspace)

    def test_semantic_successor_uses_reviewed_normal_source_write(self):
        spec = self.specification()
        source = self.root / "successor.rs"
        source.write_text("pub fn value() -> usize { 10 }\n")
        changed = dict(path="src/lib.rs", size=source.stat().st_size, sha256=INPUTS.BASE.digest(source))
        spec["files"] = sorted([item for item in spec["files"] if item["path"] != "src/lib.rs"]
                               + [changed], key=lambda item: item["path"])
        spec["inventory_sha256"] = INPUTS.BASE.BIND.inventory_sha(spec["files"])
        sources = {item["path"]: self.workspace / item["path"] for item in spec["files"]}
        sources["src/lib.rs"] = source
        INPUTS.normal_write(self.workspace, spec, sources)
        self.assertEqual((self.workspace / "src/lib.rs").read_bytes(), source.read_bytes())
        INPUTS.fixture(self.workspace, spec)

    def test_unreviewed_config_remains_rejected(self):
        spec = self.specification()
        (self.workspace / ".cargo").mkdir()
        (self.workspace / ".cargo/config.toml").write_text("[source.crates-io]\nreplace-with='evil'\n")
        environment = dict(HOME=str(self.root / "home"), CARGO_HOME=str(self.root / "cargo-home"))
        with self.assertRaisesRegex(ValueError, "unreviewed configuration"):
            INPUTS.configurations(self.workspace, environment, spec)

    def test_reviewed_config_identity_is_bound_and_mutations_reject(self):
        (self.workspace / ".cargo").mkdir()
        config = self.workspace / ".cargo/config.toml"
        config.write_text('[build]\nrustflags=["--cfg", "negative_b"]\n')
        spec = self.specification()
        env = dict(HOME=str(self.root / "home"), CARGO_HOME=str(self.root / "cargo-home"))
        self.assertTrue(any(item["status"] == "reviewed" for item in INPUTS.configurations(self.workspace, env, spec)))
        config.write_text('[build]\nrustflags=["--cfg", "different"]\n')
        with self.assertRaisesRegex(ValueError, "reviewed Cargo config differs"):
            INPUTS.configurations(self.workspace, env, spec)

    def test_failed_library_never_runs_old_observer_or_exports(self):
        with patch.object(RUN, "measured") as executed:
            with self.assertRaisesRegex(ValueError, "failed library"):
                RUN.observer(None, {}, {}, {"returncode": 1}, "10\n")
        executed.assert_not_called()

    def test_direct_root_wall_is_measured_without_linker_count_inference(self):
        with patch.object(RUN.BASE, "execute", return_value={"returncode": 0}):
            observed = RUN.measured(["/owned/rustc"], self.workspace, {}, self.root, "compiler")
        self.assertEqual(observed["observed_root_process_count"], 1)
        self.assertGreaterEqual(observed["wall_ns"], 0)
        self.assertNotIn("linker_process_count", observed)


if __name__ == "__main__":
    unittest.main()
