"""Exact T08 protocol regressions; all subprocesses simulated, no native execution."""
import argparse
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import importlib.util

ROOT = Path(__file__).resolve().parent

def load(name, filename):
    spec=importlib.util.spec_from_file_location(name,ROOT/filename)
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

RUN=load("t08_successor_test","run_negative_t08_v2.py")
HELPERS=load("t08_physical_mock","test_run_same_root.py")
TRANSPORT=load("t08_public_transport_mock","test_run_v2.py")
LOCKS=load("t08_lock_mock","test_resolve_t08.py")

class T08Tests(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name).resolve()

    def package(self,version="1.0.17"):
        checksum="92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2" if version=="1.0.17" else "8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682"
        return dict(name="itoa",version=version,source="registry+https://github.com/rust-lang/crates.io-index",checksum=checksum)

    def test_public_current_version_source_artifact_and_owned_root(self):
        artifact=self.root/"observed.rlib"
        artifact.write_bytes(b"mock artifact")
        stdout=self.root/"public.stdout"
        value=dict(reason="compiler-artifact",target=dict(name="itoa"),filenames=[str(artifact)],package_id=self.package()["source"]+"#itoa@1.0.17")
        stdout.write_text(json.dumps(value)+"\n")
        result=RUN.public_dependency(stdout,self.package(),self.root)
        self.assertEqual(result["artifacts"],[RUN.BASE.artifact(artifact)])
        for identity in (self.package()["source"]+"#itoa@1.0.18","path+file:///wrong#itoa@1.0.17"):
            value["package_id"]=identity
            stdout.write_text(json.dumps(value)+"\n")
            with self.assertRaisesRegex(ValueError,"identity differs"):
                RUN.public_dependency(stdout,self.package(),self.root)
        value["package_id"]=self.package()["source"]+"#itoa@1.0.17"
        stdout.write_text(json.dumps(value)+"\n")
        with self.assertRaisesRegex(ValueError,"escapes state"):
            RUN.public_dependency(stdout,self.package(),self.root/"absent")
        stdout.write_text((json.dumps(value)+"\n")*2)
        with self.assertRaisesRegex(ValueError,"exactly one"):
            RUN.public_dependency(stdout,self.package(),self.root)

    def setup_pipeline(self):
        sim=HELPERS.SameRootTests(methodName="test_retention_failure_preserves_original_state")
        sim.setUp()
        self.addCleanup(sim.doCleanups)
        args=sim.protocol_arguments()
        args.case="T08"
        args.registry17_archive=args.registry_home/"registry/cache/id/itoa17.crate"
        args.registry17_archive.write_bytes(b"mock17 archive")
        args.registry17_source=args.registry_home/"registry/src/id/itoa17"
        args.registry17_source.mkdir()
        (args.registry17_source/"source").write_bytes(b"mock17 source")
        args.sdk_root=sim.root/"sdk"
        args.sdk_root.mkdir()
        args.linker=None
        args.expected_sdk_inventory_sha256=RUN.BASE.write_json(sim.root/"sdk.json",[])["sha256"]
        fixtures={}
        fixture_manifest=json.loads((ROOT/"manifest.json").read_bytes())
        for name in ("a","b"):
            root=sim.root/name
            RUN.BASE.BIND.copy_fixture(RUN.BASE.BIND.fixture_source(fixture_manifest),root)
            if name=="a":
                (root/"Cargo.lock").write_text(LOCKS.LOCK)
                cargo=root/"Cargo.toml"
                cargo.write_text(cargo.read_text().replace("1.0.18","1.0.17"))
            files=RUN.BASE.BIND.inventory(root)
            fixtures[name]=dict(root=str(root),files=files,inventory_sha256=RUN.BASE.BIND.inventory_sha(files))
        observer=sim.root/"observer.rs"
        observer.write_bytes(b"mock observer")
        args.blueprint=sim.root/"blueprint.json"
        case=dict(id="T08",baseline="a",successor="b",expected17=self.package(),expected_observer_stdout="9\n",expected_build_exit=0,
                  argv=json.loads((ROOT/"manifest.json").read_bytes())["commands"][0])
        blueprint=dict(schema=1,cases=[case],fixtures=fixtures,observer=dict(RUN.BASE.artifact(observer),baseline_stdout="9\n"),frozen_mbx=dict(binary_sha256=args.mbx_sha256))
        descriptor=RUN.BASE.write_json(args.blueprint,blueprint)
        args.expected_blueprint_sha256=descriptor["sha256"]
        receipt=dict(package=self.package(),archive_member_inventory=[],local_extraction_marker={})
        positive=json.loads((ROOT/"manifest.json").read_bytes())
        sim.operations,sim.imported=[],[]
        return sim,args,blueprint,descriptor,receipt,positive

    def pipeline(self,wrong_version=False):
        sim,args,blueprint,descriptor,receipt,positive=self.setup_pipeline()
        versions=[]
        def execute(argv,cwd,env,logs,name):
            argv=[str(v) for v in argv]
            if name.startswith("observer-"):
                stdout,stderr=logs/(name+".stdout"),logs/(name+".stderr")
                stdout.write_text("9\n" if name=="observer-run" else "")
                stderr.write_bytes(b"")
                if name=="observer-compile":Path(argv[-1]).write_bytes(b"newly linked mock binary")
                return dict(argv=argv,cwd=str(cwd),environment=env,returncode=0,stdout=RUN.BASE.artifact(stdout),stderr=RUN.BASE.artifact(stderr))
            result=sim.protocol_execute(argv,cwd,env,logs,name)
            operation=RUN.NEG.V2.owned_transport(argv)
            if operation=="export":public=TRANSPORT.export_report(True)
            elif operation=="import":public=dict(version=1,actions=1,objects=1,bytes=8,comparison_state_recorded=True,workspace_restored=False,workspace_restore="skipped_incompatible")
            elif operation=="comparison-state":public=dict(version=1,valid=True,empty=True)
            else:
                lock=RUN.tomllib.loads((Path(cwd)/"Cargo.lock").read_text())
                version=next(p["version"] for p in lock["package"] if p["name"]=="itoa")
                versions.append(version)
                library=Path(cwd).parent/"target/current-fixture.rlib"
                dependency=Path(cwd).parent/"target/current-itoa.rlib"
                library.write_bytes(b"current fixture")
                dependency.write_bytes(version.encode())
                fixture=dict(reason="compiler-artifact",package_id="mock fixture",manifest_path=str(Path(cwd)/"Cargo.toml"),target=dict(name="mbx_synchronous_registry_fixture",kind=["lib"],crate_types=["lib"],src_path=str(Path(cwd)/"src/lib.rs")),filenames=[str(library)])
                dep=dict(reason="compiler-artifact",package_id=self.package()["source"]+"#itoa@"+("1.0.17" if wrong_version else version),target=dict(name="itoa"),filenames=[str(dependency)])
                public=[fixture,dep]
            Path(result["stdout"]["path"]).write_text("\n".join(json.dumps(v) for v in public)+"\n" if isinstance(public,list) else json.dumps(public)+"\n")
            result["stdout"]=RUN.BASE.artifact(Path(result["stdout"]["path"]))
            return result
        record=dict(runs=[],states=[],status="failed",native_authority=None)
        with patch.object(RUN,"sealed_blueprint",return_value=(blueprint,descriptor,descriptor)), \
             patch.object(RUN,"verify_sources",return_value=(dict(itoa17="mock",native_authority=None),receipt,positive)), \
             patch.object(RUN.BASE.BIND,"verify"),patch.object(RUN.BASE.BIND,"verify_registry"), \
             patch.object(RUN.BASE,"tool_observations"),patch.object(RUN.BASE,"execute",side_effect=execute):
            with RUN.hooks(),RUN.NEG.V2.strict_execution(RUN.BASE,RUN.NEG.PROTOCOL):
                if wrong_version:
                    with self.assertRaisesRegex(ValueError,"identity differs"):RUN.run_all(args,record)
                else:RUN.run_all(args,record)
        self.assertEqual(versions,["1.0.17","1.0.18"])
        self.assertEqual(sim.imported,[b"native export 1"])
        self.assertFalse(args.active_root.exists())
        self.assertEqual(record["status"],"failed" if wrong_version else "observed-local-negative-case")
        self.assertEqual(sim.operations,["comparison-state","export","import"]+([] if wrong_version else ["export"]))
        for run in record["runs"]:
            self.assertTrue(Path(run["commands"][0]["stdout"]["retained"]["path"]).is_file())
        if not wrong_version:
            self.assertEqual([r["observer"]["execution"]["returncode"] for r in record["runs"]],[0,0])

    def test_dual_index_checksum_record_and_config_mismatch_reject(self):
        home=self.root/"registry-home"
        index=home/"registry/index/id/.cache/it/oa/itoa"
        index.parent.mkdir(parents=True)
        config=index.parents[3]/"config.json"
        config.write_text('{"dl":"https://static.crates.io/crates"}')
        records=[dict(name="itoa",vers=v,cksum=self.package(v)["checksum"],yanked=False) for v in ("1.0.17","1.0.18")]
        raw=b"header\0"+b"\0".join(json.dumps(r).encode() for r in records)+b"\0"
        index.write_bytes(raw)
        args=argparse.Namespace(registry_home=home,source_relative=Path("registry/src/id/itoa18"),
            expected_registry_index_sha256=RUN.BASE.digest(index),expected_index_config_sha256=RUN.BASE.digest(config))
        receipt=dict(package=self.package(),registry_index=dict(selected_record=records[0]))
        positive=dict(registry=dict(archive_sha256=self.package("1.0.18")["checksum"]))
        RUN.verify_index(args,receipt,positive)
        receipt["package"]["checksum"]="0"*64
        with self.assertRaisesRegex(ValueError,"selected tuple differs"):RUN.verify_index(args,receipt,positive)
        receipt["package"]=self.package()
        receipt["registry_index"]["selected_record"]=dict(records[0],yanked=True)
        with self.assertRaisesRegex(ValueError,"index record differs"):RUN.verify_index(args,receipt,positive)
        config.write_text("mutated")
        with self.assertRaisesRegex(ValueError,"index/config differs"):RUN.verify_index(args,receipt,positive)

    def test_independent17_receipt_digest_and_tuple_reject(self):
        receipt_path=self.root/"receipt.json"
        receipt=dict(package=self.package())
        descriptor=RUN.BASE.write_json(receipt_path,receipt)
        blueprint=dict(source_receipts=dict(itoa17=descriptor),cases=[dict(id="T08",expected17=self.package())])
        args=argparse.Namespace(expected_itoa17_receipt_sha256=descriptor["sha256"])
        RUN.receipt17(args,blueprint)
        args.expected_itoa17_receipt_sha256="0"*64
        with self.assertRaisesRegex(ValueError,"source receipt differs"):RUN.receipt17(args,blueprint)
        receipt["package"]["version"]="1.0.18"
        receipt_path.unlink()
        descriptor=RUN.BASE.write_json(receipt_path,receipt)
        args.expected_itoa17_receipt_sha256=descriptor["sha256"]
        blueprint["source_receipts"]["itoa17"]=descriptor
        with self.assertRaisesRegex(ValueError,"package tuple differs"):RUN.receipt17(args,blueprint)

    def test_final_dual_guard_failure_resets_success_and_preserves_primary_error(self):
        sim,args,blueprint,descriptor,receipt,positive=self.setup_pipeline()
        record=dict(status="failed")
        def observed(*arguments):record["status"]="observed-local-negative-case"
        with patch.object(RUN,"sealed_blueprint",return_value=(blueprint,descriptor,descriptor)), \
             patch.object(RUN,"verify_sources",side_effect=[(dict(mock=True),receipt,positive),ValueError("mutated receipt")]), \
             patch.object(RUN.NEG,"execute_case",side_effect=observed):
            with self.assertRaisesRegex(ValueError,"after guard failed"):RUN.run_all(args,record)
        self.assertEqual(record["status"],"failed")
        self.assertIn("mutated receipt",record["dual_source_after_guard"]["error"])
        with patch.object(RUN,"sealed_blueprint",return_value=(blueprint,descriptor,descriptor)), \
             patch.object(RUN,"verify_sources",side_effect=[(dict(mock=True),receipt,positive),ValueError("mutated receipt")]), \
             patch.object(RUN.NEG,"execute_case",side_effect=ValueError("primary compiler failure")):
            with self.assertRaisesRegex(ValueError,"primary compiler failure"):RUN.run_all(args,record)
        self.assertEqual(record["status"],"failed")
        self.assertIn("mutated receipt",record["dual_source_after_guard"]["error"])

    def test_exact_input_hooks_restore_after_any_failure(self):
        original=(RUN.NEG.arguments,RUN.NEG.run_all,RUN.NEG.input_receipt,RUN.BASE.seed_registry)
        for cause in ("failed17 receipt","failed18 receipt","failed final guard"):
            with self.subTest(cause=cause):
                with self.assertRaisesRegex(ValueError,cause):
                    with RUN.hooks():
                        self.assertIs(RUN.NEG.input_receipt,RUN.input_receipt)
                        self.assertIs(RUN.BASE.seed_registry,RUN.seed_registry)
                        raise ValueError(cause)
                self.assertEqual((RUN.NEG.arguments,RUN.NEG.run_all,RUN.NEG.input_receipt,RUN.BASE.seed_registry),original)

    def test_full_two_state_exact17_to18_with_equal_observer(self):
        self.pipeline()

    def test_stale17_public_artifact_cannot_satisfy_current18(self):
        self.pipeline(wrong_version=True)
