"""Operation failures close the issuer recorded at private issuance time.

The NativeOwner and frame are synthetic issuer seams. Temporary roots, manifests,
OriginalFS state and descriptor closure are real; this suite makes no host or
Foundation qualification claim.
"""
import os
import sys
import tempfile
import unittest
from pathlib import Path


ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_semantics_leases as leases
import source_semantics_lifetime as lifetime
import source_semantics_sdk as semantics_sdk
import source_semantics_witness_test as witness_fixtures
from source_intent_cold_common import ColdSourceIntent
from source_intent_native_semantics_validation import NativeSourceSemanticsUnavailable
from source_semantics_leases_test import FixtureAcquired, _source_tree


class CloseTracker:
    """Foreign parent that records an illicit cleanup call."""

    def __init__(self):
        self.closed = False

    def close(self):
        self.closed = True


def _reset_runtime():
    witness_fixtures._reset_runtime()
    for name in ("_REGISTRY_RECORDS", "_ENTRY_RECORDS", "_INVOCATION_RECORDS"):
        value = getattr(leases, name, None)
        if isinstance(value, dict):
            value.clear()


class SourceSemanticsOwnedCleanupTests(unittest.TestCase):
    def tearDown(self):
        _reset_runtime()

    def _assert_original_closed(self, owner, current, sdk, state, descriptor):
        self.assertFalse(owner.live)
        self.assertFalse(owner.executor.live)
        self.assertNotIn(id(sdk), semantics_sdk._SDKS)
        self.assertEqual(semantics_sdk._BOUND_SDKS, {})
        self.assertEqual(leases._REGISTRIES, {})
        self.assertEqual(leases._REGISTRY_RECORDS, {})
        self.assertIsNone(state._descriptor)
        with self.assertRaises(OSError):
            os.fstat(descriptor)
        self.assertEqual(lifetime._LIVE_REGISTRY, {})
        with self.assertRaises(ColdSourceIntent):
            current.require_current()

    def test_parent_slot_tamper_closes_issued_original_parent(self):
        with tempfile.TemporaryDirectory(prefix="velnor-owned-cleanup-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            helper = witness_fixtures.SourceSemanticsWitnessTests()
            with helper._configured((acquired,)) as (
                    owner, current, registry, sdk, _executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                operand = operation.source_operand()
                state = registry._snapshots[0]._state
                descriptor = state._descriptor
                foreign = CloseTracker()
                object.__setattr__(operation, "_parent", foreign)
                with self.assertRaises(NativeSourceSemanticsUnavailable):
                    operation.manifest_read(operand)
                self.assertFalse(foreign.closed)
                self._assert_original_closed(owner, current, sdk, state, descriptor)

    def test_invocation_slot_tamper_closes_issued_original_parent(self):
        with tempfile.TemporaryDirectory(prefix="velnor-owned-cleanup-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            helper = witness_fixtures.SourceSemanticsWitnessTests()
            with helper._configured((acquired,)) as (
                    owner, current, registry, sdk, _executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                operand = operation.source_operand()
                state = registry._snapshots[0]._state
                descriptor = state._descriptor
                object.__setattr__(operation, "_invocation", object())
                with self.assertRaises(NativeSourceSemanticsUnavailable):
                    operation.manifest_read(operand)
                self._assert_original_closed(owner, current, sdk, state, descriptor)


if __name__ == "__main__":
    unittest.main()
