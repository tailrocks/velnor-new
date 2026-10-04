"""Configuration-only context checks; no source, SDK, or compiler grants minted."""
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1] / "src"


class PreparedContextTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.output = self.root / "runner-output"
        self.output.write_text("")
        self.ns = {"__name__": "velnor_release_compiled"}
        for name in ("release_reconcile_common.py", "release_source_intent_output.py",
                     "release_source_intent_context.py"):
            source = ROOT / name
            exec(compile(source.read_bytes(), str(source), "exec"), self.ns)
        self.configuration = {
            "approved_json": json.dumps({"schema": 1, "packages": {"one": "1.0.0"}}),
            "release_config": "[workspace]\nrelease = false\n",
            "manifest": "nested/Cargo.toml", "actual_host": "x86_64-unknown-linux-gnu"}

    def context(self):
        # Configuration injection simulates frozen owner bytes only. It supplies
        # no authenticated source, installed tool receipt, or workflow approval.
        self.ns["_COMPILED_PREPARED_CONFIGURATION"] = self.configuration
        with patch.dict(os.environ, {"RUNNER_TEMP": str(self.root),
                                     "GITHUB_OUTPUT": str(self.output)}, clear=True):
            return self.ns["compiled_prepared_context"]()

    def test_default_cold_denial_before_environment_access(self):
        with patch.dict(os.environ, {}, clear=True), \
                self.assertRaisesRegex(self.ns["ReconcileError"], "unqualified"):
            self.ns["compiled_prepared_context"]()
        self.assertEqual(self.output.read_text(), "")

    def test_frozen_config_cached_context_and_exact_destination(self):
        context = self.context()
        self.assertIs(context, self.context())
        self.assertEqual(context.destination,
                         self.root / "velnor/source-intent-prepared/prepared.zip")
        self.assertEqual(context.manifest, "nested/Cargo.toml")
        self.assertEqual(context.release_config, self.configuration["release_config"])
        self.assertEqual(context.actual_host, "x86_64-unknown-linux-gnu")
        changed = context.approved
        changed["packages"]["one"] = "wrong"
        self.assertEqual(context.approved["packages"]["one"], "1.0.0")
        self.configuration["manifest"] = "changed"
        self.assertEqual(context.manifest, "nested/Cargo.toml")
        with self.assertRaises(AttributeError):
            context.actual_host = "other"
        with self.assertRaises(AttributeError):
            context.output_sink._emitted = False
        digest = context.write_prepared_payload(b"immutable payload")
        self.assertEqual(digest, hashlib.sha256(b"immutable payload").hexdigest())
        self.assertEqual(context.destination.read_bytes(), b"immutable payload")
        context.output_sink.publish_prepared_sha256(digest)
        self.assertEqual(self.output.read_text(), "package-blob-sha256=" + digest + "\n")
        with self.assertRaisesRegex(self.ns["ReconcileError"], "repeated"):
            context.output_sink.publish_prepared_sha256("b" * 64)

    def test_unknown_missing_and_nonstring_configuration_rejected(self):
        for mutation in (lambda value: value.update(extra="claim"),
                         lambda value: value.pop("release_config"),
                         lambda value: value.update(actual_host=True),
                         lambda value: value.update(manifest="")):
            original = dict(self.configuration)
            mutation(self.configuration)
            with self.assertRaises(self.ns["ReconcileError"]):
                self.context()
            self.configuration = original
        self.assertEqual(self.output.read_text(), "")

    def test_boolean_policy_schema_rejected(self):
        self.configuration["approved_json"] = '{"schema":true}'
        with self.assertRaisesRegex(self.ns["ReconcileError"], "policy"):
            self.context()

    def test_invalid_digest_never_emitted(self):
        original = self.root
        for index, digest in enumerate((True, "A" * 64, "a" * 64 + "\nextra=value")):
            self.root = original / f"case{index}"
            self.root.mkdir(mode=0o700)
            self.ns["_PREPARED_CONTEXT"] = None
            context = self.context()
            context.write_prepared_payload(b"immutable payload")
            with self.assertRaisesRegex(self.ns["ReconcileError"], "digest"):
                context.output_sink.publish_prepared_sha256(digest)
        self.assertEqual(self.output.read_text(), "")

    def test_missing_output_channel_denies_without_creation(self):
        self.output.unlink()
        with self.assertRaises(FileNotFoundError):
            self.context()
        self.assertFalse(self.output.exists())

    def test_symlink_output_denied(self):
        outside = self.root / "other-output"
        outside.write_text("")
        self.output.unlink()
        self.output.symlink_to(outside)
        with self.assertRaises(OSError):
            self.context()
        self.assertEqual(outside.read_text(), "")

    def test_private_type_constructor_requires_seal(self):
        with self.assertRaisesRegex(self.ns["ReconcileError"], "capability"):
            self.ns["_CompiledPreparedContext"](None, self.configuration, str(self.root))
        with self.assertRaisesRegex(self.ns["ReconcileError"], "capability"):
            self.ns["_PreparedOutputSink"](None, self.root)


if __name__ == "__main__":
    unittest.main()
