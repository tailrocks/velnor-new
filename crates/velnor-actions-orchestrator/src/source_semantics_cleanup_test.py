"""Cleanup and held-slot regressions; issuer fixtures are not qualification."""
import os
import sys
import tempfile
import unittest
from contextlib import contextmanager, ExitStack
from pathlib import Path
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_sdk as cold_sdk
import source_semantics_inventory as inventory
import source_semantics_leases as leases
import source_semantics_lifetime as lifetime
import source_semantics_sdk as semantics_sdk
from source_intent_cold_common import ColdSourceIntent
from source_intent_native_semantics_validation import NativeSourceSemanticsUnavailable
from source_semantics_leases_test import (
    FixtureAcquired, FixtureExecutor, FixtureGoverning, FixtureOwner, _source_tree,
)


class FixtureWitness:
    """Synthetic original witness used only to exercise registry revocation."""

    def __init__(self):
        self.live = True

    def require_current(self):
        if not self.live:
            raise NativeSourceSemanticsUnavailable("fixture_witness_revoked")

    def _revoke(self):
        self.live = False


def _sdk_fixture(lifetime_lease, registry):
    sdk = object.__new__(semantics_sdk.SourceSemanticsSdk)
    object.__setattr__(sdk, "_lifetime", lifetime_lease)
    object.__setattr__(sdk, "_registry", registry)
    semantics_sdk._SDKS[id(sdk)] = (sdk, lifetime_lease, registry)
    sdk.require_current()
    return sdk


def _clear_map(module, *names):
    for name in names:
        value = getattr(module, name, None)
        if isinstance(value, dict):
            value.clear()


def _reset_runtime():
    _clear_map(semantics_sdk, "_SDKS", "_BOUND_SDKS")
    _clear_map(leases, "_REGISTRIES", "_ENTRIES", "_INVOCATIONS", "_USED_LIFETIMES",
               "_REGISTRY_RECORDS", "_ENTRY_RECORDS", "_INVOCATION_RECORDS")
    _clear_map(lifetime, "_LIVE_REGISTRY")
    lifetime._LIVE_LIFETIME = None


class SourceSemanticsCleanupTests(unittest.TestCase):
    def tearDown(self):
        _reset_runtime()

    @contextmanager
    def _configured(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cleanup-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            owner = FixtureOwner(object.__new__(cold_sdk.ColdSourceIntentSdk),
                                 FixtureExecutor(), (acquired,))
            stack = ExitStack()
            stack.enter_context(patch.object(cold_sdk.ColdSourceIntentSdk,
                                              "require_current", return_value=None))
            bindings = (
                (lifetime, {"_COMPILED_SOURCE_SEMANTICS_OWNER_TYPE": type(owner),
                            "_COMPILED_SOURCE_SEMANTICS_OWNER_GETTER": lambda: owner,
                            "_COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE": type(owner.executor)}),
                (leases, {"_COMPILED_PREPARATION_OWNER_TYPE": type(owner),
                          "_COMPILED_PREPARATION_OWNER_GETTER": lambda: owner,
                          "_COMPILED_ACQUIRED_SNAPSHOT_TYPE": type(acquired)}),
            )
            if hasattr(leases, "_COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE"):
                bindings[1][1]["_COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE"] = FixtureGoverning
            for module, values in bindings:
                for name, value in values.items():
                    stack.enter_context(patch.object(module, name, value))
            current = registry = sdk = None
            try:
                current = lifetime.load_source_sdk_lifetime()
                owner.lifetime = current
                registry = leases._load_snapshot_registry(current)
                sdk = _sdk_fixture(current, registry)
                yield owner, current, registry, sdk, manifest
            finally:
                if sdk is not None and id(sdk) in semantics_sdk._SDKS:
                    try:
                        sdk.close()
                    except BaseException:
                        pass
                elif registry is not None and id(registry) in leases._REGISTRIES:
                    registry.close()
                if current is not None and id(current) in lifetime._LIVE_REGISTRY:
                    try:
                        current.close()
                    except BaseException:
                        pass
                stack.close()
                _reset_runtime()

    def test_held_slots_deny_and_close_private_originals(self):
        slots = (
            ("sdk-lifetime", "sdk", "_lifetime", object()),
            ("sdk-registry", "sdk", "_registry", object()),
            ("lifetime-owner", "lifetime", "_owner", object()),
            ("registry-owner", "registry", "_owner", object()),
            ("registry-snapshots", "registry", "_snapshots", ()),
            ("registry-invocations", "registry", "_invocations", []),
            ("entry-state", "entry", "_state", object()),
            ("entry-manifests", "entry", "_manifests", ()),
            ("invocation-identity", "invocation", "_identity", object()),
            ("invocation-operation", "invocation", "_operation", "foreign"),
        )
        for label, target_name, name, replacement in slots:
            with self.subTest(slot=label), self._configured() as (
                    owner, current, registry, sdk, manifest):
                entry = registry._snapshots[0]
                invocation = registry._select(manifest, "manifest_read")
                target = {"sdk": sdk, "lifetime": current, "registry": registry,
                          "entry": entry, "invocation": invocation}[target_name]
                state = entry._state
                descriptor = state._descriptor
                object.__setattr__(target, name, replacement)
                check = getattr(target, "require_current")
                with self.assertRaises((NativeSourceSemanticsUnavailable,
                                        ColdSourceIntent)):
                    check()
                sdk.close()
                self.assertIsNone(state._descriptor)
                with self.assertRaises(OSError):
                    os.fstat(descriptor)
                self.assertFalse(owner.live)
                self.assertEqual(lifetime._LIVE_REGISTRY, {})
                self.assertEqual(leases._REGISTRY_RECORDS, {})
                self.assertEqual(leases._ENTRY_RECORDS, {})
                self.assertEqual(leases._INVOCATION_RECORDS, {})

    def test_sdk_close_revokes_witnesses_and_closes_original_state_fd(self):
        with self._configured() as (owner, current, registry, sdk, manifest):
            entry = registry._snapshots[0]
            state = entry._state
            descriptor = state._descriptor
            witness = FixtureWitness()
            entry._witnesses[manifest] = witness
            sdk.close()
            self.assertFalse(witness.live)
            with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "revoked"):
                witness.require_current()
            self.assertIsNone(state._descriptor)
            with self.assertRaises(OSError):
                os.fstat(descriptor)
            with self.assertRaises(NativeSourceSemanticsUnavailable):
                entry.require_current()
            with self.assertRaises(ColdSourceIntent):
                current.require_current()
            self.assertFalse(owner.live)

    def test_lifetime_owner_closes_when_registry_cleanup_reports_error(self):
        with self._configured() as (owner, current, registry, sdk, _manifest):
            state = registry._snapshots[0]._state
            original_close = inventory._OriginalSourceState.close

            def close_then_fail(value):
                original_close(value)
                raise RuntimeError("fixture cleanup subcall")

            with patch.object(inventory._OriginalSourceState, "close",
                              side_effect=close_then_fail, autospec=True), \
                    self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                           "source_sdk_cleanup_failed"):
                sdk.close()
            self.assertIsNone(state._descriptor)
            self.assertFalse(owner.live)
            self.assertEqual(lifetime._LIVE_REGISTRY, {})
            with self.assertRaises(ColdSourceIntent):
                current.require_current()


if __name__ == "__main__":
    unittest.main()
