"""Dedicated verifier absence denies before semantic configuration or source work."""
from pathlib import Path
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1] / "src"


class VerificationContextTest(unittest.TestCase):
    def setUp(self):
        self.ns = {"__name__": "compiled_verification_fixture"}
        for name in ("release_reconcile_common.py",
                     "release_source_intent_verification_context.py"):
            source = ROOT / name
            exec(compile(source.read_bytes(), str(source), "exec"), self.ns)

    def test_missing_owner_denies_before_configuration_parser(self):
        with patch.dict(self.ns, {"decode_json": lambda _: self.fail("configuration accessed")}):
            with self.assertRaisesRegex(self.ns["ReconcileError"],
                                        "verification_native_owner_unavailable"):
                self.ns["compiled_verification_context"]()

    def test_actual_native_owner_missing_role_denies_before_filesystem(self):
        native = {"__name__": "actual_native_verifier_fixture"}
        source = ROOT / "source_intent_native_verifier.py"
        exec(compile(source.read_bytes(), str(source), "exec"), native)
        for name in ("NativeVerifierSdk", "load_native_verifier_sdk"):
            self.ns[name] = native[name]
        with patch.object(native["os"], "open", side_effect=AssertionError("filesystem")):
            with self.assertRaisesRegex(native["NativeVerifierUnavailable"],
                                        "native_verifier_authority_unavailable"):
                self.ns["compiled_verification_context"]()

    def test_wrong_owner_result_never_becomes_configuration_authority(self):
        self.ns["NativeVerifierSdk"] = type("DiagnosticSdkType", (), {})
        self.ns["load_native_verifier_sdk"] = lambda: object()
        with self.assertRaisesRegex(self.ns["ReconcileError"],
                                    "verification_native_owner_capability"):
            self.ns["compiled_verification_context"]()

    def test_public_json_cannot_construct_context(self):
        configuration = {"approved_json": '{"schema":1}', "manifest": "Cargo.toml",
                         "actual_host": "x86_64-unknown-linux-gnu"}
        with self.assertRaisesRegex(self.ns["ReconcileError"],
                                    "verification_context_capability"):
            self.ns["_CompiledVerificationContext"](object(), configuration, object())

    def test_owned_tool_is_closed_when_context_construction_fails(self):
        native = {"__name__": "native_cleanup_diagnostic"}
        source = ROOT / "source_intent_native_verifier.py"
        exec(compile(source.read_bytes(), str(source), "exec"), native)
        sdk_type = native["NativeVerifierSdk"]
        # A diagnostic object exercises cleanup only; it bypasses no live issuer.
        tool = object.__new__(sdk_type)
        self.ns.update(NativeVerifierSdk=sdk_type, load_native_verifier_sdk=lambda: tool)
        for configuration in (None, {"unknown": "field"}):
            self.ns["_COMPILED_VERIFICATION_CONFIGURATION"] = configuration
            with patch.object(sdk_type, "require_current"), \
                    patch.object(sdk_type, "close") as close:
                with self.assertRaises(self.ns["ReconcileError"]):
                    self.ns["compiled_verification_context"]()
                close.assert_called_once()
                self.assertIsNone(self.ns["_VERIFICATION_CONTEXT"])

    def test_acquired_current_failure_closes_tool(self):
        native = {"__name__": "native_current_diagnostic"}
        source = ROOT / "source_intent_native_verifier.py"
        exec(compile(source.read_bytes(), str(source), "exec"), native)
        sdk_type = native["NativeVerifierSdk"]
        tool = object.__new__(sdk_type)
        self.ns.update(NativeVerifierSdk=sdk_type, load_native_verifier_sdk=lambda: tool)
        with patch.object(sdk_type, "require_current", side_effect=RuntimeError("changed")), \
                patch.object(sdk_type, "close") as close:
            with self.assertRaisesRegex(RuntimeError, "changed"):
                self.ns["compiled_verification_context"]()
            close.assert_called_once()
            self.assertIsNone(self.ns["_VERIFICATION_CONTEXT"])

    def test_cached_failure_is_cleared_even_when_close_fails(self):
        native = {"__name__": "native_cached_diagnostic"}
        source = ROOT / "source_intent_native_verifier.py"
        exec(compile(source.read_bytes(), str(source), "exec"), native)
        sdk_type = native["NativeVerifierSdk"]
        tool = object.__new__(sdk_type)
        context = object.__new__(self.ns["_CompiledVerificationContext"])
        object.__setattr__(context, "native_toolcap", tool)
        self.ns.update(NativeVerifierSdk=sdk_type, load_native_verifier_sdk=lambda: tool,
                       _VERIFICATION_CONTEXT=context)
        with patch.object(sdk_type, "require_current", side_effect=RuntimeError("changed")), \
                patch.object(sdk_type, "close", side_effect=OSError("close")) as close:
            with self.assertRaisesRegex(OSError, "close"):
                self.ns["compiled_verification_context"]()
            close.assert_called_once()
            self.assertIsNone(self.ns["_VERIFICATION_CONTEXT"])

    def test_native_installation_close_failure_clears_descriptor_once(self):
        native = {"__name__": "native_descriptor_diagnostic"}
        source = ROOT / "source_intent_native_verifier.py"
        exec(compile(source.read_bytes(), str(source), "exec"), native)
        installed = object.__new__(native["_NativeVerifierInstallation"])
        object.__setattr__(installed, "_descriptor", 123)
        with patch.object(native["os"], "close", side_effect=OSError("close")) as close:
            with self.assertRaisesRegex(OSError, "close"):
                installed.close()
            self.assertIsNone(installed._descriptor)
            installed.close()
            close.assert_called_once_with(123)


if __name__ == "__main__":
    unittest.main()
