"""Prepared entry contract tests with mocked private capabilities only."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import zipfile


ROOT = Path(__file__).resolve().parents[1] / "src"


def namespace():
    result = {"__name__": "prepared_entry_tests"}
    for name in ("release_source_intent_guard.py", "release_source_intent_cargo.py",
                 "release_source_intent_contract.py", "release_source_intent_prepare.py"):
        path = ROOT / name
        exec(compile(path.read_bytes(), str(path), "exec"), result)
    return result


class MockPreparedContext(SimpleNamespace):
    def write_prepared_payload(self, data):
        self.destination.parent.mkdir(parents=True, exist_ok=True)
        with self.destination.open("xb") as output:
            output.write(data)
        return hashlib.sha256(data).hexdigest()


class MockPreparedSink:
    def __init__(self):
        self.digests = []

    def publish_prepared_sha256(self, digest):
        self.digests.append(digest)


class MockColdSdk:
    def require_policy(self, rust, host):
        if (rust, host) != ("1.98.1", "x86_64-unknown-linux-gnu"):
            raise ValueError("mock cold policy mismatch")


class PreparedEntryTests(unittest.TestCase):
    def setUp(self):
        self.ns = namespace()
        self.temporary = tempfile.TemporaryDirectory(dir=Path("/tmp").resolve())
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def entry_dependencies(self):
        approved = {"tools": {"rust": "1.98.1"}, "packages": {"demo": "1.0.0"}}
        sink = MockPreparedSink()
        context = MockPreparedContext(approved=approved, manifest="Cargo.toml",
            actual_host="x86_64-unknown-linux-gnu", release_config="[workspace]\n",
            destination=self.directory / "prepared/prepared.zip", output_sink=sink)
        source = object()
        original = b"official mock original bytes"
        package = {"publish_metadata": {"name": "demo", "vers": "1.0.0"},
                   "archive_sha256": hashlib.sha256(original).hexdigest()}

        def inventory(policy, manifest, sdk, host, destination, release_config, source_descriptor):
            path = destination / "crates"
            path.mkdir()
            (path / "demo-1.0.0.crate").write_bytes(original)
            return {"demo": package}

        self.ns.update(ColdSourceIntentSdk=MockColdSdk, ColdSourceIntentInstalledTool=Mock,
            load_source_intent_sdk=lambda: MockColdSdk(), _CompiledPreparedContext=MockPreparedContext,
            _PreparedOutputSink=MockPreparedSink, compiled_prepared_context=lambda: context,
            load_authenticated_source_snapshot=lambda: source,
            authenticated_source_descriptor=lambda value: {"mock_source": "not authority"},
            source_intent_inventory=inventory, selected_publication_order=lambda packages: ["demo"],
            validate_prepared_evidence=Mock())
        return context, original, package

    def test_missing_cold_type_or_loader_precedes_all_side_effects(self):
        loader, write = Mock(), Mock()
        self.ns.update(load_authenticated_source_snapshot=loader, _prepared_write=write,
                       compiled_prepared_context=Mock())
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "ColdSourceIntentSdk"):
            self.ns["prepare_source_intent"]()
        self.ns.update(ColdSourceIntentSdk=MockColdSdk, ColdSourceIntentInstalledTool=Mock)
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "load_source_intent_sdk"):
            self.ns["prepare_source_intent"]()
        self.ns["load_source_intent_sdk"] = Mock(side_effect=ValueError("cold qualification missing"))
        with self.assertRaisesRegex(ValueError, "cold qualification missing"):
            self.ns["prepare_source_intent"]()
        loader.assert_not_called()
        write.assert_not_called()
        self.ns["compiled_prepared_context"].assert_not_called()

    def test_actual_compiled_context_default_denies_ambient_output(self):
        context = ROOT.parents[1] / "velnor-actions-orchestrator/src/release_source_intent_context.py"
        self.ns.update(require=self.ns["_intent_require"], ReconcileError=self.ns["SourceIntentError"],
                       decode_json=json.loads)
        output = context.with_name("release_source_intent_output.py")
        exec(compile(output.read_bytes(), str(output), "exec"), self.ns)
        exec(compile(context.read_bytes(), str(context), "exec"), self.ns)
        self.ns.update(ColdSourceIntentSdk=MockColdSdk, ColdSourceIntentInstalledTool=Mock,
                       load_source_intent_sdk=Mock(return_value=MockColdSdk()),
                       load_authenticated_source_snapshot=Mock())
        with patch.dict("os.environ", {"GITHUB_OUTPUT": str(self.directory / "hostile")}):
            with self.assertRaisesRegex(self.ns["SourceIntentError"], "prepared_context_unqualified"):
                self.ns["prepare_source_intent"]()
        self.assertFalse((self.directory / "hostile").exists())
        self.ns["load_source_intent_sdk"].assert_called_once_with()

    def test_zeroarg_entry_preserves_same_bytes_and_fixed_sha_output(self):
        context, original, package = self.entry_dependencies()
        result = self.ns["prepare_source_intent"]()
        self.assertEqual(context.destination.read_bytes(), result.data)
        self.assertEqual(context.output_sink.digests, [hashlib.sha256(result.data).hexdigest()])
        with zipfile.ZipFile(io.BytesIO(result.data)) as archive:
            self.assertEqual(set(archive.namelist()), {"evidence.json", "crates/demo-1.0.0.crate"})
            self.assertEqual(archive.read("crates/demo-1.0.0.crate"), original)
            evidence = json.loads(archive.read("evidence.json"))
            self.assertEqual(set(evidence), self.ns["PREPARED_EVIDENCE_FIELDS"])
            self.assertEqual(evidence["packages"], {"demo": package})
            self.assertEqual(evidence["kind"], "source-intent-prepared")
            self.assertTrue(all(item.compress_type == zipfile.ZIP_STORED for item in archive.infolist()))
        with self.assertRaises(AttributeError):
            result.data = b"substitution"

    def test_zip_deterministic_and_shared_bounds_checked(self):
        first = self.ns["_prepared_zip"]({"schema": 1}, {"crates/a.crate": b"a"})
        second = self.ns["_prepared_zip"]({"schema": 1}, {"crates/a.crate": b"a"})
        self.assertEqual(first.data, second.data)
        with patch.dict(self.ns, {"PREPARED_MAX_ARCHIVE_BYTES": 0}):
            with self.assertRaisesRegex(self.ns["SourceIntentError"], "original_bounds"):
                self.ns["_prepared_zip"]({}, {"crates/a.crate": b"a"})

    def test_original_digest_symlink_and_extra_coverage_rejected(self):
        context, original, package = self.entry_dependencies()
        crates = self.directory / "crates"
        crates.mkdir()
        path = crates / "demo-1.0.0.crate"
        path.write_bytes(b"substituted")
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "original_digest"):
            self.ns["_prepared_originals"]({"demo": package}, self.directory)
        path.unlink()
        path.symlink_to(self.directory / "target")
        with self.assertRaises(OSError):
            self.ns["_prepared_originals"]({"demo": package}, self.directory)
        path.unlink()
        path.write_bytes(original)
        (crates / "extra").write_bytes(b"extra")
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "original_coverage"):
            self.ns["_prepared_originals"]({"demo": package}, self.directory)

    def test_mock_output_cap_rejects_existing_payload_and_emits_no_sha(self):
        context, _, _ = self.entry_dependencies()
        context.destination.parent.mkdir()
        context.destination.write_bytes(b"old")
        with self.assertRaises(FileExistsError):
            self.ns["prepare_source_intent"]()
        self.assertEqual(context.destination.read_bytes(), b"old")
        self.assertEqual(context.output_sink.digests, [])

    def test_custody_digest_mismatch_never_publishes(self):
        context, _, _ = self.entry_dependencies()
        context.write_prepared_payload = Mock(return_value="0" * 64)
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "output_payload_digest"):
            self.ns["prepare_source_intent"]()
        context.write_prepared_payload.assert_called_once()
        self.assertEqual(context.output_sink.digests, [])


if __name__ == "__main__":
    unittest.main()
