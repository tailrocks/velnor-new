"""Bootstrap validator tests use synthetic TOML only; never create a corpus lock."""

import argparse
import json
from unittest.mock import patch
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("resolve_t08_test", Path(__file__).with_name("resolve_t08.py"))
RUN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUN)

LOCK = """version = 4
[[package]]
name = "mbx-synchronous-registry-fixture"
version = "0.0.0"
dependencies = ["itoa"]
[[package]]
name = "itoa"
version = "1.0.17"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2"
"""


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.lock = self.root / "synthetic-parser-test.lock"
        self.package = dict(name="itoa", version="1.0.17",
                            source="registry+https://github.com/rust-lang/crates.io-index",
                            checksum="92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2")

    def test_exact_actual_tuple_validator_shape(self):
        self.lock.write_text(LOCK)
        result = RUN.generated_lock(self.lock, self.package)
        self.assertEqual(result["package"][1], self.package)

    def test_generated_lock_version_requires_integer_schema(self):
        self.lock.write_text(LOCK.replace("version = 4", "version = 4.0"))
        with self.assertRaisesRegex(ValueError,"lock schema differs"):
            RUN.generated_lock(self.lock,self.package)

    def test_changed_checksum_version_or_origin_reject(self):
        for old, new in (("1.0.17", "1.0.18"),
                         (self.package["checksum"], "0" * 64),
                         (self.package["source"], "registry+https://untrusted.example/index")):
            with self.subTest(mutation=old):
                self.lock.write_text(LOCK.replace(old, new))
                with self.assertRaisesRegex(ValueError, "registry resolution differs"):
                    RUN.generated_lock(self.lock, self.package)

    def test_extra_resolved_package_is_not_silently_accepted(self):
        self.lock.write_text(LOCK + '\n[[package]]\nname="unexpected"\nversion="1.0.0"\n')
        with self.assertRaisesRegex(ValueError, "unreviewed package closure"):
            RUN.generated_lock(self.lock, self.package)

    def test_registry_review_requires_independent_receipt_digest(self):
        path = self.root / "receipt.json"
        path.write_text('{"native_authority": null}')
        with self.assertRaisesRegex(ValueError, "input digest differs"):
            RUN.reviewed(path, "0" * 64)

    def bootstrap_pipeline(self, failure):
        tools = self.root / "tools"
        (tools / "bin").mkdir(parents=True)
        seed = self.root / "seed"
        seed.mkdir()
        (seed / "input").write_bytes(b"original seed")
        fixture = self.root / "fixture"
        (fixture / "src").mkdir(parents=True)
        (fixture / "Cargo.toml").write_bytes(b"mock fixture manifest")
        (fixture / "src/lib.rs").write_bytes(b"mock source")
        output = self.root / "out"
        output.mkdir()
        args = argparse.Namespace(output=output, registry_home=seed, toolchain_root=tools,
            blueprint=self.root / "blueprint.json", source_receipt=self.root / "source.json",
            archive_relative=Path("registry/archive"), source_relative=Path("registry/source"), group="mock")
        args.expected_blueprint_sha256 = RUN.BASE.write_json(args.blueprint,
            dict(fixtures={"t08-itoa17":dict(root=str(fixture))}))["sha256"]
        RUN.BASE.write_json(args.source_receipt,{"mock":True})
        for name in ("cargo","rustc"):
            path = tools / "bin" / name
            path.write_bytes(name.encode())
            setattr(args,name,path)
            setattr(args,name+"_sha256",RUN.BASE.digest(path))
        args.expected_toolchain_inventory_sha256 = RUN.BASE.write_json(
            self.root / "tools.json",RUN.BASE.tree(tools))["sha256"]
        spec = dict(root=str(fixture))
        receipt = dict(package=self.package,archive_member_inventory=[],local_extraction_marker={})
        reviewed = (dict(),spec,receipt,RUN.BASE.artifact(args.blueprint),RUN.BASE.artifact(args.source_receipt))
        def seed_registry(args,destination):
            (destination / "registry").mkdir()
            (destination / "registry/input").write_bytes(b"copied input")
        def execute(argv,cwd,env,logs,name):
            stdout,stderr=logs/(name+".stdout"),logs/(name+".stderr")
            stdout.write_text("cargo 1.98.1 mock\n" if name=="cargo-version" else "")
            stderr.write_text("actual mock failure")
            if name != "cargo-version" and not failure:
                (cwd/"Cargo.lock").write_text(LOCK)
            if name != "cargo-version" and failure:
                (cwd/"Cargo.lock").symlink_to(cwd/"src/lib.rs")
                (seed/"input").write_bytes(b"mutated original seed")
                args.rustc.write_bytes(b"mutated tool")
                (cwd/".cargo").mkdir()
                (cwd/".cargo/config.toml").write_bytes(b"mutated config")
            return dict(argv=[str(v) for v in argv],cwd=str(cwd),environment=env,
                returncode=0 if name=="cargo-version" or not failure else 23,
                stdout=RUN.BASE.artifact(stdout),stderr=RUN.BASE.artifact(stderr))
        record={"status":"failed"}
        with patch.object(RUN,"initial_inputs",return_value=reviewed), \
             patch.object(RUN.BASE,"seed_registry",side_effect=seed_registry), \
             patch.object(RUN.BASE,"execute",side_effect=execute):
            if failure:
                with self.assertRaisesRegex(ValueError,"offline lock creation failed"):
                    RUN.bootstrap_with_guards(args,record)
            else:
                RUN.bootstrap_with_guards(args,record)
        if not failure:
            self.assertEqual(record["status"],"generated-awaiting-independent-review")
            self.assertEqual(record["resolved_packages"][1],self.package)
            self.assertTrue(all(g["status"]=="unchanged" for g in record["after_guards"]))
            self.assertFalse((fixture/"Cargo.lock").exists())
            return
        self.assertEqual(record["command"]["returncode"],23)
        self.assertTrue(Path(record["command"]["stderr"]["path"]).is_file())
        guards={item["name"]:item for item in record["after_guards"]}
        for name in ("origin-registry","distribution","rustc"):
            self.assertEqual(guards[name]["status"],"changed")
        self.assertEqual(guards["config"]["status"],"unavailable")
        self.assertEqual(guards["fixture-sources"]["status"],"unavailable")
        self.assertTrue((output/"bootstrap/workspace/Cargo.lock").is_symlink())

    def test_full_bootstrap_failure_preserves_code_and_unconditional_guards(self):
        self.bootstrap_pipeline(True)

    def test_full_mock_success_keeps_real_corpus_untouched(self):
        self.bootstrap_pipeline(False)

    def test_failure_symlink_cannot_suppress_durable_bootstrap_record(self):
        output=self.root/"main-output"
        args=argparse.Namespace(output=output)
        def failure(args,record):
            (output/"raw.stderr").write_bytes(b"retained failure")
            (output/"invalid.lock").symlink_to(output/"raw.stderr")
            record["after_guards"]=[dict(name="fixture",status="unavailable",error="symlink")]
            raise ValueError("mock Cargo failed")
        with patch.object(RUN,"arguments",return_value=args), \
             patch.object(RUN,"bootstrap_with_guards",side_effect=failure),patch("builtins.print"):
            self.assertEqual(RUN.main(),1)
        record=json.loads((output/"bootstrap.json").read_bytes())
        self.assertEqual(record["error"],"mock Cargo failed")
        self.assertEqual(record["symlink_anomalies"],[str(output/"invalid.lock")])
        self.assertEqual(len(record["artifacts"]),1)


if __name__ == "__main__":
    unittest.main()
