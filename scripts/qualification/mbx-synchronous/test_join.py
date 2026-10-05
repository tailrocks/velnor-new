"""Pure evidence checks: bytes/identities bind; parsed flags cannot grant authority."""
import argparse
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile
import unittest
import uuid
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("fixture_join", Path(__file__).with_name("join.py"))
JOIN = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = JOIN
SPEC.loader.exec_module(JOIN)
SESSION = "12345678-1234-4234-8234-123456789abc"

RUN_SPEC = importlib.util.spec_from_file_location("joined_runner", Path(__file__).with_name("run.py"))
RUN = importlib.util.module_from_spec(RUN_SPEC)
RUN_SPEC.loader.exec_module(RUN)


class JoinTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()

    def artifact(self, name, data):
        path = self.root / name
        path.write_bytes(data)
        return dict(path=str(path), size=len(data), sha256=JOIN.sha(data))

    def command(self):
        identity = dict(session_id=SESSION, root_session_id=SESSION,
                        parent_session_id=None, caller_correlation="attempt:1:command-1",
                        command_role="cargo_build")
        report = dict(schema_version=1, completed=True, mbx_version="1.21.0",
                      source_base_version="1.21.0", identity=identity,
                      workload=dict(outcome="succeeded", exit_code=0),
                      statistics=dict(measurement=dict(workload_wall_ns=12,
                                      link_wall_ns=None, coverage=dict(status="unknown")),
                                      local_qualification=dict(local_status="local_evidence_complete")))
        descriptor = self.artifact(SESSION + ".json", json.dumps(report).encode())
        return dict(argv=["/mbx", "build", "--lib"], returncode=0,
                    report_artifacts=[descriptor]), report

    def test_original_statistics_and_unknown_remain(self):
        command, original = self.command()
        observed = JOIN.native_reports(command, "attempt:1:command-1", set())
        self.assertEqual(observed[0]["statistics"], original["statistics"])
        self.assertIsNone(observed[0]["statistics"]["measurement"]["link_wall_ns"])
        self.assertEqual(observed[0]["statistics"]["measurement"]["coverage"]["status"], "unknown")

    def test_replayed_native_session_rejects(self):
        command, _ = self.command()
        with self.assertRaisesRegex(ValueError, "replayed native session"):
            JOIN.native_reports(command, "attempt:1:command-1", {SESSION})

    def test_correlation_mismatch_rejects(self):
        command, _ = self.command()
        with self.assertRaisesRegex(ValueError, "correlation differs"):
            JOIN.native_reports(command, "another-attempt", set())

    def test_command_role_mismatch_rejects(self):
        command, _ = self.command()
        command["argv"][1] = "check"
        with self.assertRaisesRegex(ValueError, "command role differs"):
            JOIN.native_reports(command, "attempt:1:command-1", set())

    def test_changed_artifact_rejects(self):
        descriptor = self.artifact("bytes", b"observed")
        Path(descriptor["path"]).write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            JOIN.artifact(descriptor)

    def test_symlink_artifact_rejects(self):
        descriptor = self.artifact("bytes", b"observed")
        alias = self.root / "alias"
        alias.symlink_to(Path(descriptor["path"]))
        descriptor["path"] = str(alias)
        with self.assertRaisesRegex(ValueError, "canonical artifact"):
            JOIN.artifact(descriptor)

    def test_bounded_artifact_rejects(self):
        descriptor = self.artifact("bytes", b"observed")
        with self.assertRaisesRegex(ValueError, "invalid artifact path/size"):
            JOIN.artifact(descriptor, 2)

    def test_bool_schema_and_duplicate_json_reject(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            JOIN.document(b'{"schema":1,"schema":true}')
        command, original = self.command()
        original["schema_version"] = True
        command["report_artifacts"] = [self.artifact(SESSION + ".json", json.dumps(original).encode())]
        with self.assertRaisesRegex(ValueError, "unsupported native schema"):
            JOIN.native_reports(command, "attempt:1:command-1", set())

    def test_inputs_only_receipt_cannot_be_native_report(self):
        descriptor = self.artifact("binder.json", b'{"native_authority":null,"schema":1}')
        with self.assertRaisesRegex(ValueError, "unsupported native schema"):
            JOIN.native_reports(dict(report_artifacts=[descriptor]), "attempt", set())

    def test_raw_flags_cannot_promote_broad_native_scope(self):
        command, _ = self.command()
        observed = JOIN.native_reports(command, "attempt:1:command-1", set())
        self.assertNotIn("native_authority", observed[0])
        lifetime = JOIN.LifetimeEvidence(JOIN.SCOPE, "unknown", "unknown", "rustc_library_only",
                                        "accepted_before_close_only", "unknown", ())
        self.assertEqual(lifetime.generic_taskwide_status, "unknown")
        self.assertEqual(lifetime.status, "unknown")

    def test_source_receipt_requires_independent_digest(self):
        descriptor = self.artifact("source.json", b'{"qualified":true}')
        with self.assertRaisesRegex(ValueError, "independent digest"):
            JOIN.retained_sources(dict(mbx=descriptor, compiler=None), {})
        # Binding reviewed bytes still supplies no native capability.
        result = JOIN.retained_sources(dict(mbx=descriptor, compiler=None),
                                       dict(mbx=descriptor["sha256"]))
        self.assertEqual(result["mbx"], descriptor)

    def test_distribution_inventory_real_size_uses_separate_bound(self):
        # Actual retained Rust inventory measured 13,251,203 bytes; native reports stay 8 MiB.
        raw = b"[]" + b" " * (13_251_203 - 2)
        descriptor = self.artifact("distribution-inventory.json", raw)
        directory = self.root / "empty-distribution"
        directory.mkdir()
        JOIN.inventory(descriptor, directory)
        with self.assertRaisesRegex(ValueError, "invalid artifact path/size"):
            JOIN.artifact(descriptor, JOIN.MAX_REPORT_BYTES)
        descriptor["size"] = JOIN.MAX_INVENTORY_BYTES + 1
        with self.assertRaisesRegex(ValueError, "invalid artifact path/size"):
            JOIN.inventory(descriptor, directory)

    def test_nonfinite_json_rejects(self):
        with self.assertRaisesRegex(ValueError, "nonfinite"):
            JOIN.document(b'{"metric":NaN}')


class RunnerJoinTests(JoinTests):
    def prepare_runner(self):
        source = self.root / "source"
        source.mkdir()
        original = Path(__file__).resolve().parent
        manifest = json.loads((original / "manifest.json").read_bytes())
        shutil.copytree(RUN.BIND.fixture_source(manifest), source / "registry-fixture")
        manifest["fixture"]["root"] = "registry-fixture"
        seed = self.root / "seed"
        extracted = seed / "registry/src/registry/tiny-1.0.0"
        extracted.mkdir(parents=True)
        records = []
        archive = seed / "registry/cache/registry/tiny-1.0.0.crate"
        archive.parent.mkdir(parents=True)
        with tarfile.open(archive, "w:gz") as stream:
            for name in ("src/lib.rs", "src/u128_ext.rs"):
                data = b"// synthetic pure join test source\n"
                path = extracted / name
                path.parent.mkdir(exist_ok=True)
                path.write_bytes(data)
                member = tarfile.TarInfo("tiny-1.0.0/" + name)
                member.size = len(data)
                stream.addfile(member, io.BytesIO(data))
                records.append(dict(path=name, size=len(data), sha256=JOIN.sha(data)))
        (extracted / ".cargo-ok").write_bytes(b"")
        index = seed / "registry/index/registry"
        (index / ".cache/it/oa").mkdir(parents=True)
        (index / "config.json").write_text("{}")
        (index / ".cache/it/oa/itoa").write_bytes(b"synthetic index")
        manifest["registry"] = dict(name="tiny", version="1.0.0", files=records,
            archive_sha256=RUN.digest(archive), inventory_sha256=RUN.BIND.inventory_sha(records),
            local_extraction_marker=dict(path=".cargo-ok", size=0, sha256=JOIN.sha(b"")))
        (source / "manifest.json").write_text(json.dumps(manifest))
        toolchain = self.root / "toolchain"
        (toolchain / "bin").mkdir(parents=True)
        for name in ("cargo", "rustc", "mbx"):
            (toolchain / "bin" / name).write_bytes(("synthetic " + name).encode())
        output = self.root / "output"
        output.mkdir()
        args = argparse.Namespace(output=output, toolchain_root=toolchain, registry_home=seed,
            archive_relative=archive.relative_to(seed), source_relative=extracted.relative_to(seed),
            expected_manifest_sha256=RUN.digest(source / "manifest.json"), group="fixture-test",
            mbx_source_receipt=None, mbx_source_receipt_sha256=None, compiler_source_receipt=None,
            compiler_source_receipt_sha256=None, prequalified_local=True)
        for name in ("cargo", "rustc", "mbx"):
            setattr(args, name, toolchain / "bin" / name)
            setattr(args, name + "_sha256", RUN.digest(getattr(args, name)))
        return args, source

    def fake_execute(self, argv, cwd, env, logs, name):
        argv = [str(value) for value in argv]
        output = b"synthetic version\n"
        if argv[1] in ("build", "check"):
            session = str(uuid.uuid4())
            identity = dict(session_id=session, root_session_id=session, parent_session_id=None,
                            caller_correlation=env["MBX_REPORT_CORRELATION_ID"],
                            command_role="cargo_" + argv[1])
            report = dict(schema_version=1, completed=True, mbx_version="test",
                source_base_version="test", identity=identity, workload=dict(outcome="succeeded", exit_code=0),
                statistics=dict(measurement=dict(workload_wall_ns=7, link_wall_ns=None,
                                coverage=dict(status="unknown")),
                                local_qualification=dict(local_status="local_evidence_complete")))
            RUN.write_json(Path(env["MBX_STATS_REPORT_DIR"]) / (session + ".json"), report)
            receipt = dict(schema_version=1, mbx_version="test", source_base_version="test",
                identity={key: value for key, value in identity.items() if key != "command_role"},
                event_id=uuid.uuid4().hex, adapter="rustc", event_kind="process",
                delivery=dict(status="acknowledged", event=None, event_sha256=None))
            RUN.write_json(Path(env["MBX_STATS_REPORT_DIR"]) / (session + ".measurement-ack.json"), receipt)
            ledger = Path(env["MBX_STATS_REPORT_DIR"]) / (".mbx-admissions-" + session)
            ledger.mkdir()
            for filename in ("identity.json", "closed.json", ".lock",
                             receipt["event_id"] + ".accepted.json", receipt["event_id"] + ".terminal.json"):
                # Native ledger internals lack public schema_version; lock need not be JSON.
                (ledger / filename).write_bytes(b"opaque lock\xff" if filename == ".lock"
                                                else b'{"native_private":"sealed"}')
        elif argv[1:3] == ["--print", "sysroot"]:
            output = (str(Path(argv[0]).parent.parent) + "\n").encode()
        elif argv[1:3] == ["cache", "comparison-state"]:
            Path(argv[3]).write_text("{}")
        elif argv[1:3] == ["cache", "import"]:
            Path(argv[5]).write_text("{}")
            shutil.rmtree(argv[3])
        elif argv[1:3] == ["cache", "export"]:
            exported = Path(cwd).parent.name == "run-1"
            if exported:
                Path(argv[3]).mkdir()
                (Path(argv[3]) / "native-object").write_bytes(b"original owned bytes")
            output = json.dumps(dict(exported=exported)).encode()
        stdout, stderr = logs / (name + ".stdout"), logs / (name + ".stderr")
        stdout.write_bytes(output)
        stderr.write_bytes(b"")
        return dict(argv=argv, cwd=str(cwd), environment=dict(env), returncode=0,
                    stdout=RUN.artifact(stdout), stderr=RUN.artifact(stderr))

    def actual_record(self):
        args, source = self.prepare_runner()
        record = dict(schema=1, scope=JOIN.SCOPE, execution_kind="local", run_attempt_id=args.group,
                      status="failed", runs=[], native_authority=None, native_abi=None,
                      hosted_t01_t03=None, host=dict(system="test", release="test", machine="test"),
                      limitations=[])
        with patch.object(RUN, "ROOT", source), patch.object(RUN.BIND, "ROOT", source), \
                patch.object(RUN, "execute", side_effect=self.fake_execute):
            RUN.run_all(args, record)
        record["artifacts"] = [RUN.artifact(path) for path in sorted(args.output.rglob("*"))
                               if path.is_file() and "logs" in str(path.parent)]
        return record, RUN.artifact(source / "manifest.json")

    def test_actual_runner_record_joins_six_reports_as_unknown(self):
        record, manifest = self.actual_record()
        descriptor = self.artifact("execution.json", json.dumps(record).encode())
        result = JOIN.join(descriptor, manifest)
        self.assertEqual(result.status, "unknown")
        self.assertIsNone(result.native_authority)
        self.assertEqual(result.lifetime.compiler_scope, "unknown")
        self.assertEqual(result.lifetime.generic_taskwide_status, "unknown")
        self.assertEqual(len(result.observed_reports), 6)
        self.assertTrue(all(item["receipts"][0]["receipt"]["event_id"] for item in result.observed_reports))
        self.assertTrue(all(len(item["opaque_admission_artifacts"]) == 5 for item in result.observed_reports))
        self.assertEqual(record["runs"][0]["environment"]["CARGO_BUILD_JOBS"], "2")
        self.assertTrue(all(item["statistics"]["measurement"]["link_wall_ns"] is None
                            for item in result.observed_reports))
        self.assertFalse(record["runs"][1]["transport"][1]["export_observed"]["exported"])
        self.assertFalse(record["runs"][2]["transport"][0]["consumed_copy_exists_after"])

    def test_unsupported_native_root_artifact_rejects(self):
        record, manifest = self.actual_record()
        command = record["runs"][0]["commands"][0]
        ledger = next(item for item in command["admission_artifacts"]
                      if Path(item["path"]).name == "identity.json")
        root = Path(command["environment"]["MBX_STATS_REPORT_DIR"])
        unsupported = root / "internal-at-root.json"
        unsupported.write_bytes(Path(ledger["path"]).read_bytes())
        command["report_artifacts"].append(RUN.artifact(unsupported))
        descriptor = self.artifact("execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "unsupported native schema"):
            JOIN.join(descriptor, manifest)

    def test_admission_owner_mismatch_rejects(self):
        record, manifest = self.actual_record()
        command = record["runs"][0]["commands"][0]
        command["admission_artifacts"] = record["runs"][0]["commands"][1]["admission_artifacts"]
        descriptor = self.artifact("execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "subpath|opaque admission artifact owner differs"):
            JOIN.join(descriptor, manifest)

    def test_native_boolean_flag_cannot_promote_runner_record(self):
        record, manifest = self.actual_record()
        record["native_authority"] = True
        descriptor = self.artifact("execution.json", json.dumps(record).encode())
        with self.assertRaisesRegex(ValueError, "replayed native authority flag"):
            JOIN.join(descriptor, manifest)


if __name__ == "__main__":
    unittest.main()
