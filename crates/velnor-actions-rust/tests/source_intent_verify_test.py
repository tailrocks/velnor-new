"""Native authority refusal and pure original-archive custody checks, offline."""
import copy
import hashlib
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1] / "src"
COMMON = ROOT.parents[1] / "velnor-actions-orchestrator" / "src"
FIXTURE = {"__file__": str(Path(__file__).with_name("release_publish_test_fixture.py"))}
exec(compile(Path(FIXTURE["__file__"]).read_text(), "package_fixture", "exec"), FIXTURE)


def namespace():
    result = {"__name__": "source_intent_verify_test"}
    for source in (COMMON / "release_reconcile_common.py", ROOT / "release_source_validation.py",
                   COMMON / "source_intent_native_verifier.py",
                   COMMON / "release_source_intent_verification_context.py",
                   ROOT / "release_source_intent_verify.py"):
        exec(compile(source.read_text(), str(source), "exec"), result)
    return result


class NativeAuthorityTests(unittest.TestCase):
    def setUp(self):
        self.ns = namespace()

    def test_actual_entry_missing_native_role_precedes_every_effect(self):
        forbidden = Mock(side_effect=AssertionError("effect before native qualification"))
        self.assertIsNone(self.ns["_COMPILED_NATIVE_VERIFIER_SOURCE_ROLE"])
        observers = {name: forbidden for name in ("load_authenticated_prepared_package",
            "load_authenticated_source_snapshot", "authenticated_prepared_package",
            "materialize_authenticated_source_snapshot", "decode_json")}
        with patch.dict(self.ns, observers), \
                patch.object(self.ns["tempfile"], "TemporaryDirectory", forbidden), \
                patch.object(self.ns["subprocess"], "Popen", forbidden), \
                patch.object(self.ns["subprocess"], "run", forbidden), \
                patch.object(self.ns["os"], "open", forbidden), \
                patch.object(Path, "resolve", forbidden), \
                patch.dict(os.environ, {"RUNNER_TEMP": "/caller-controlled", "GH_TOKEN": "ambient"}):
            with self.assertRaisesRegex(self.ns["NativeVerifierUnavailable"], "native_verifier_authority_unavailable"):
                self.ns["verify_source_intent"]()
        forbidden.assert_not_called()
        self.assertIsNone(self.ns["_VERIFICATION_CONTEXT"])

    def test_session_constructor_wrong_seal_rejects_before_prepared_or_files(self):
        forbidden = Mock(side_effect=AssertionError("session touched unqualified input"))
        with patch.dict(self.ns, {"authenticated_prepared_package": forbidden}), \
                patch.object(self.ns["tempfile"], "TemporaryDirectory", forbidden), \
                patch.object(self.ns["os"], "open", forbidden):
            with self.assertRaisesRegex(self.ns["ReconcileError"], "verification_session_authority"):
                self.ns["_OriginalArchiveVerificationSession"](object(), object(), "Cargo.toml")
        forbidden.assert_not_called()

    def test_dict_path_and_duck_session_reject_before_files_or_tool_checks(self):
        forbidden = Mock(side_effect=AssertionError("session touched filesystem"))
        duck = SimpleNamespace(root_path="/source", require_current=forbidden)
        with patch.object(self.ns["os"], "open", forbidden), patch.object(Path, "lstat", forbidden):
            for operand in ({"root_path": "/source"}, Path("/source"), duck, object()):
                with self.subTest(operand=type(operand).__name__), \
                        self.assertRaisesRegex(self.ns["ReconcileError"], "verification_session_authority"):
                    self.ns["validate_original_archive_verification_session"](operand)
                with self.assertRaisesRegex(self.ns["NativeVerifierUnavailable"], "original_session_authority"):
                    self.ns["_original_session"](operand, duck, False)
        forbidden.assert_not_called()

    def test_unsealed_orphan_cannot_mutate_slots_or_reach_filesystem(self):
        session_type = self.ns["_OriginalArchiveVerificationSession"]
        orphan = object.__new__(session_type)
        forbidden = Mock(side_effect=AssertionError("orphan session touched filesystem"))
        for slot in session_type.__slots__:
            with self.subTest(slot=slot), self.assertRaises(AttributeError):
                setattr(orphan, slot, object())
        with patch.object(self.ns["os"], "open", forbidden), \
                patch.object(Path, "lstat", forbidden), \
                patch.dict(self.ns, {"authenticated_prepared_package": forbidden,
                                     "authenticated_source_snapshot": forbidden}):
            with self.assertRaisesRegex(self.ns["ReconcileError"], "verification_session_authority"):
                self.ns["validate_original_archive_verification_session"](orphan)
        forbidden.assert_not_called()

    def test_native_sdk_constructor_rejects_stock_lookalike_and_private_fake_seal(self):
        stock = SimpleNamespace(installed_tool=Mock(), require_current=Mock(), host="x86_64-unknown-linux-gnu")
        forbidden = Mock(side_effect=AssertionError("unqualified sdk touched filesystem"))
        with patch.object(self.ns["os"], "open", forbidden):
            for operand in (stock, {"path": "/usr/bin/cargo"}, Path("/usr/bin/cargo"), object()):
                with self.subTest(operand=type(operand).__name__), \
                        self.assertRaisesRegex(self.ns["NativeVerifierUnavailable"], "sdk_origin"):
                    self.ns["NativeVerifierSdk"](operand, _seal=object())
        forbidden.assert_not_called()
        stock.require_current.assert_not_called()
        stock.installed_tool.assert_not_called()


class OriginalIntentTests(unittest.TestCase):
    def setUp(self):
        self.ns = namespace()
        self.package, self.archive = FIXTURE["package"]()
        self.evidence = {"publication_order": ["demo"], "packages": {"demo": self.package}}

    def test_exact_original_archive_hash_paths_and_feature_flags(self):
        intent = self.ns["original_archive_session_intent"](FIXTURE["POLICY"], self.evidence, "nested/Cargo.toml")
        self.assertEqual(intent, {"format": 1, "source_manifest": "snapshot/nested/Cargo.toml",
            "targets": [], "prepared": None, "archives": [{"package_name": "demo",
                "package_version": "1.0.0", "archive_path": "archives/demo-1.0.0.crate",
                "sha256": hashlib.sha256(self.archive).hexdigest(), "features": [],
                "all_features": False, "no_default_features": False}]})
        self.assertEqual(self.evidence["packages"]["demo"], self.package)

    def test_actual_relative_manifest_validator_rejects_unsafe_inputs(self):
        for manifest in ("../Cargo.toml", "/Cargo.toml", "a//Cargo.toml", "./Cargo.toml",
                         "a/../Cargo.toml", "a\\Cargo.toml", "other.toml", ""):
            with self.subTest(manifest=manifest), self.assertRaises(self.ns["ValidationError"]):
                self.ns["original_archive_session_intent"](FIXTURE["POLICY"], self.evidence, manifest)

    def test_publication_order_is_preserved_in_claimed_native_archive_list(self):
        second, _archive = FIXTURE["package"]("second")
        evidence = {"publication_order": ["second", "demo"], "packages": {"demo": self.package, "second": second}}
        policy = copy.deepcopy(FIXTURE["POLICY"])
        policy["packages"]["second"] = "1.0.0"
        intent = self.ns["original_archive_session_intent"](policy, evidence, "Cargo.toml")
        self.assertEqual([item["package_name"] for item in intent["archives"]], ["second", "demo"])


class VerificationClosureTests(unittest.TestCase):
    def setUp(self):
        self.ns = namespace()
        self.temporary = tempfile.TemporaryDirectory(dir=Path(tempfile.gettempdir()).resolve())
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.file = self.root / "original.crate"
        self.file.write_bytes(b"fixed original bytes")

    def test_regular_closure_records_exact_digest_and_changes_after_edit(self):
        before = self.ns["_verification_closure"](self.root)
        self.assertEqual(before["original.crate"][1], hashlib.sha256(self.file.read_bytes()).hexdigest())
        self.file.write_bytes(b"changed original bytes")
        self.assertNotEqual(before, self.ns["_verification_closure"](self.root))

    def test_symlinks_to_files_and_directories_are_rejected(self):
        directory = self.root / "directory"
        directory.mkdir()
        for target in (self.file, directory):
            link = self.root / "link"
            link.symlink_to(target, target_is_directory=target.is_dir())
            with self.subTest(target=target.name), \
                    self.assertRaisesRegex(self.ns["ReconcileError"], "verification_closure_symlink"):
                self.ns["_verification_closure"](self.root)
            link.unlink()

    def test_fifo_and_hardlinked_file_reject_without_reading(self):
        fifo = self.root / "fifo"
        os.mkfifo(fifo)
        read = Mock(side_effect=AssertionError("read nonregular input"))
        with patch.object(self.ns["os"], "read", read), \
                self.assertRaisesRegex(self.ns["ReconcileError"], "verification_closure_file"):
            self.ns["_verification_file"](fifo)
        read.assert_not_called()
        fifo.unlink()
        link = self.root / "hardlink"
        os.link(self.file, link)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "verification_closure_file"):
            self.ns["_verification_closure"](self.root)

    def test_mutation_during_actual_file_read_is_rejected(self):
        read = os.read
        changed = False

        def mutate(descriptor, length):
            nonlocal changed
            data = read(descriptor, length)
            if data and not changed:
                changed = True
                self.file.write_bytes(b"mutation during read")
            return data

        with patch.object(self.ns["os"], "read", side_effect=mutate), \
                self.assertRaisesRegex(self.ns["ReconcileError"], "verification_closure_changed"):
            self.ns["_verification_file"](self.file)

    def test_count_total_and_single_file_resource_limits(self):
        for constant, value, reason in (("_VERIFICATION_MAX_FILES", 0, "verification_closure_count"),
                                       ("_VERIFICATION_MAX_BYTES", 1, "verification_closure_size")):
            with self.subTest(constant=constant), patch.dict(self.ns, {constant: value}), \
                    self.assertRaisesRegex(self.ns["ReconcileError"], reason):
                self.ns["_verification_closure"](self.root)
        with self.file.open("r+b") as stream:
            stream.truncate(1024 * 1024 * 1024 + 1)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "verification_closure_file"):
            self.ns["_verification_file"](self.file)


if __name__ == "__main__":
    unittest.main()
