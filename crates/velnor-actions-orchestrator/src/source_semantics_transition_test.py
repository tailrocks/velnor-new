"""Synthetic transition tests over the real OriginalFS observer.

The owners below are test-only source qualification seams. They do not prove a
Foundation, installer, host, or native Cargo qualification.
"""
import sys
import tempfile
import unittest
from contextlib import contextmanager
from pathlib import Path
from types import MethodType
from unittest.mock import patch


ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_sdk as cold_sdk
import source_semantics_edited as edited
import source_semantics_leases as leases
import source_semantics_manifest_read as manifest_read
import source_semantics_sdk as semantics_sdk
from source_archive_inventory_common import InventoryError
from source_intent_native_semantics_validation import NativeSourceSemanticsUnavailable
from source_semantics_leases_test import FixtureGoverning, _raw_manifest, _source_tree
import source_semantics_witness_test as witness_fixtures
from source_semantics_witness_test import (
    EditedAcquired,
    EditedOwner,
    NativeManifestExecutor,
    NativeManifestFrame,
)


class FixturePending:
    """Exact owner handle for one pending lock transition."""

    __slots__ = ("owner", "acquired", "invocation", "frame", "before_raw", "live")

    def __init__(self, owner, acquired, invocation, frame, before_raw):
        self.owner = owner
        self.acquired = acquired
        self.invocation = invocation
        self.frame = frame
        self.before_raw = before_raw
        self.live = True

    def require_current(self):
        if not self.live or self.owner.pending is not self:
            raise NativeSourceSemanticsUnavailable("fixture_pending_expired")


class FixtureCompletion:
    """Exact executor handle; response bytes are observation only."""

    __slots__ = ("executor", "transition", "frame", "response_bytes", "live")

    def __init__(self, executor, transition, frame, response_bytes=b'{"ok":true}'):
        self.executor = executor
        self.transition = transition
        self.frame = frame
        self.response_bytes = response_bytes
        self.live = True

    def require_current(self):
        if not self.live or self.executor.completion is not self:
            raise NativeSourceSemanticsUnavailable("fixture_completion_expired")


def _is_current(value):
    try:
        value.require_current()
    except (InventoryError, NativeSourceSemanticsUnavailable):
        return False
    return True


def _attach_transition_owner(owner, executor, edited_root, original_manifest, edited_manifest):
    owner.edited_root = edited_root
    owner.original_manifest = original_manifest
    owner.edited_manifest = edited_manifest
    owner.pending = None
    owner.successor = None
    owner.begin_calls = owner.complete_calls = owner.pending_checks = 0
    owner.begin_saw_invocation_current = False
    owner.begin_saw_edited_current = False

    def begin(self, acquired, invocation, frame):
        self.begin_calls += 1
        self.begin_saw_invocation_current = _is_current(invocation)
        self.begin_saw_edited_current = _is_current(invocation._snapshot._edited)
        if not self.begin_saw_invocation_current or not self.begin_saw_edited_current:
            raise NativeSourceSemanticsUnavailable("fixture_inputs_retired_too_early")
        if acquired is not self.derived_acquired or frame is not self.original_frame:
            raise NativeSourceSemanticsUnavailable("fixture_begin_inputs")
        pending = FixturePending(self, acquired, invocation, frame,
                                 _raw_manifest(self.edited_root))
        self.pending = pending
        return pending

    def require_pending(self, pending, invocation, frame):
        self.pending_checks += 1
        if (pending is not self.pending or invocation is not pending.invocation
                or frame is not self.original_frame):
            raise NativeSourceSemanticsUnavailable("fixture_pending_binding")
        pending.require_current()

    def complete(self, pending, completion, before, frame):
        self.complete_calls += 1
        self.require_pending_lock_transition(pending, pending.invocation, frame)
        if (completion is not executor.completion or before != pending.before_raw
                or frame is not self.original_frame):
            raise NativeSourceSemanticsUnavailable("fixture_completion_binding")
        completion.require_current()
        successor = EditedAcquired(self.edited_root, self.edited_manifest,
                                   self.original_manifest)
        self.derived_acquired = successor
        self.successor = successor
        pending.live = False
        return successor

    def require_successor(self, acquired, pending, completion):
        if (acquired is not self.successor or pending is not self.pending
                or completion is not executor.completion):
            raise NativeSourceSemanticsUnavailable("fixture_lock_successor")

    owner.begin_governing_lock_transition = MethodType(begin, owner)
    owner.require_pending_lock_transition = MethodType(require_pending, owner)
    owner.complete_governing_lock_transition = MethodType(complete, owner)
    owner.require_lock_successor = MethodType(require_successor, owner)
    executor.owner = owner
    executor.edited_root = edited_root
    executor.mutation_calls = executor.completion_checks = 0
    executor.completion = None

    def mutate(self, transition, frame):
        self.mutation_calls += 1
        transition.require_current()
        if frame is not self.owner.original_frame:
            raise NativeSourceSemanticsUnavailable("fixture_mutation_frame")
        path = Path(self.edited_root) / "Cargo.toml"
        path.write_bytes(path.read_bytes() + b"# transition\n")
        completion = FixtureCompletion(self, transition, frame)
        self.completion = completion
        return completion

    def require_completion(self, completion, transition, frame):
        self.completion_checks += 1
        if (completion is not self.completion or completion.transition is not transition
                or completion.frame is not frame):
            raise NativeSourceSemanticsUnavailable("fixture_completion_handle")
        completion.require_current()
        transition.require_current()

    executor.workspace_version_lock_mutation = MethodType(mutate, executor)
    executor.require_mutation_completion = MethodType(require_completion, executor)


class SourceSemanticsTransitionTests(unittest.TestCase):
    """Only synthetic owner handles are used; OriginalFS is real."""

    @contextmanager
    def _transition_fixture(self):
        with tempfile.TemporaryDirectory(prefix="velnor-transition-") as directory:
            original_root = _source_tree(directory, "original")
            edited_root = _source_tree(directory, "edited")
            original_manifest = str(original_root / "Cargo.toml")
            edited_manifest = str(edited_root / "Cargo.toml")
            original = EditedAcquired(original_root, original_manifest, original_manifest)
            derived = EditedAcquired(edited_root, edited_manifest, original_manifest)
            executor = NativeManifestExecutor()
            helper = witness_fixtures.SourceSemanticsWitnessTests()
            with patch.object(edited, "_COMPILED_NATIVE_LOCK_TRANSITION_TYPE",
                              FixturePending), \
                    patch.object(semantics_sdk, "_COMPILED_NATIVE_LOCK_COMPLETION_TYPE",
                                 FixtureCompletion), \
                    helper._configured((original, derived), executor,
                                       owner_type=EditedOwner) as values:
                owner, current, registry, sdk, executor = values
                read = sdk._bind_native_request(original_manifest, "manifest_read")
                read_operand = read.source_operand()
                read.manifest_read(read_operand)
                witness = registry._snapshots[0]._witnesses[original_manifest]
                owner.original_frame = witness._frame
                owner.original = witness
                edited_lease = sdk._admit_native_edited_scratch(witness)
                governing = FixtureGoverning()
                owner.governing = governing
                mutation = sdk._bind_native_request(
                    edited_manifest, "workspace_version_lock_mutation", governing)
                _attach_transition_owner(owner, executor, edited_root,
                                         original_manifest, edited_manifest)
                yield (owner, current, registry, sdk, executor, witness, edited_lease,
                       mutation, governing, original_manifest, edited_manifest)

    def test_manifest_witness_record_blocks_slot_swaps_before_native(self):
        for slot, replacement in (("_frame", "frame"), ("_invocation", object()),
                                  ("_response", b"forged")):
            with self.subTest(slot=slot), tempfile.TemporaryDirectory(
                    prefix="velnor-witness-swap-") as directory:
                root = _source_tree(directory)
                manifest = str(root / "Cargo.toml")
                executor = NativeManifestExecutor()
                helper = witness_fixtures.SourceSemanticsWitnessTests()
                with helper._configured((
                        EditedAcquired(root, manifest, manifest),), executor) as (
                            owner, _current, registry, sdk, executor):
                    operation = sdk._bind_native_request(manifest, "manifest_read")
                    operand = operation.source_operand()
                    operation.manifest_read(operand)
                    witness = registry._snapshots[0]._witnesses[manifest]
                    record = manifest_read._WITNESS_RECORDS.get(id(witness))
                    self.assertIsNotNone(record)
                    self.assertIs(record[0], operand)
                    self.assertIs(record[3], witness._frame)
                    self.assertIs(record[5], witness._response)
                    if slot == "_frame":
                        replacement = NativeManifestFrame(witness._response, operand)
                    object.__setattr__(witness, slot, replacement)
                    with self.assertRaisesRegex(
                            NativeSourceSemanticsUnavailable, "held_binding_changed"):
                        operation.manifest_read(operand)
                    self.assertEqual(executor.manifest_calls, 1)
                    self.assertFalse(owner.live)

    def test_witness_has_no_context_authority_slot(self):
        with tempfile.TemporaryDirectory(prefix="velnor-witness-context-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            helper = witness_fixtures.SourceSemanticsWitnessTests()
            with helper._configured((EditedAcquired(root, manifest, manifest),)) as (
                    _owner, _current, registry, sdk, _executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                operand = operation.source_operand()
                operation.manifest_read(operand)
                witness = registry._snapshots[0]._witnesses[manifest]
                self.assertFalse(hasattr(witness, "_context"))
                with self.assertRaises(AttributeError):
                    object.__setattr__(witness, "_context", object())

    def test_distinct_genuine_witness_cannot_cross_snapshot_maps(self):
        with tempfile.TemporaryDirectory(prefix="velnor-witness-cross-") as directory:
            first_root = _source_tree(directory, "first")
            second_root = _source_tree(directory, "second")
            first_manifest = str(first_root / "Cargo.toml")
            second_manifest = str(second_root / "Cargo.toml")
            executor = NativeManifestExecutor()
            helper = witness_fixtures.SourceSemanticsWitnessTests()
            acquisitions = (EditedAcquired(first_root, first_manifest, first_manifest),
                            EditedAcquired(second_root, second_manifest, second_manifest))
            with helper._configured(acquisitions, executor) as (
                    owner, _current, registry, sdk, executor):
                first = sdk._bind_native_request(first_manifest, "manifest_read")
                second = sdk._bind_native_request(second_manifest, "manifest_read")
                first_operand = first.source_operand()
                second_operand = second.source_operand()
                first.manifest_read(first_operand)
                second.manifest_read(second_operand)
                first_entry = registry._snapshots[0]
                first_witness = first_entry._witnesses[first_manifest]
                second_witness = registry._snapshots[1]._witnesses[second_manifest]
                first_entry._witnesses[first_manifest] = second_witness
                with self.assertRaisesRegex(
                        NativeSourceSemanticsUnavailable,
                        "source_snapshot_witness_binding_changed"):
                    first.manifest_read(first_operand)
                self.assertEqual(executor.manifest_calls, 2)
                self.assertNotIn(id(sdk), semantics_sdk._SDKS)
                self.assertFalse(owner.live)
                self.assertFalse(executor.live)
                with self.assertRaises(NativeSourceSemanticsUnavailable):
                    first_witness.require_current()

    def test_entry_attachment_records_reject_ducks_and_close_sdk(self):
        class FakeEdited:
            def __init__(self, original):
                self._original = original

        class FakeWitness:
            def __init__(self, original):
                self._invocation = original._invocation

            def _revoke(self):
                return None

        for tamper in ("edited", "witness", "clear"):
            with self.subTest(tamper=tamper), self._transition_fixture() as values:
                owner, _current, registry, sdk, executor, witness, edited_lease, mutation, governing, *_ = values
                operand = mutation.source_operand()
                if tamper == "edited":
                    entry = edited_lease._entry
                    object.__setattr__(entry, "_edited", FakeEdited(witness))
                else:
                    entry = witness._invocation._snapshot
                    if tamper == "witness":
                        entry._witnesses[entry._manifests[0]] = FakeWitness(witness)
                    else:
                        entry._witnesses.clear()
                with self.assertRaisesRegex(
                        NativeSourceSemanticsUnavailable,
                        "source_snapshot_(edited|witness)_binding_changed"):
                    mutation.workspace_version_lock_mutation(operand, governing)
                self.assertEqual(executor.mutation_calls, 0)
                self.assertEqual(executor.completion_checks, 0)
                self.assertNotIn(id(sdk), semantics_sdk._SDKS)
                self.assertFalse(owner.live)
                self.assertFalse(executor.live)
                with self.assertRaises(NativeSourceSemanticsUnavailable):
                    witness.require_current()

    def test_begin_complete_retires_old_leases_and_keeps_original(self):
        with self._transition_fixture() as values:
            (owner, _current, registry, sdk, executor, witness, edited_lease,
             mutation, _governing, _original_manifest, edited_manifest) = values
            operand = mutation.source_operand()
            transition = edited._begin_lock_transition(operand)
            self.assertIs(type(transition._pending), FixturePending)
            self.assertEqual(transition._before, transition._pending.before_raw)
            self.assertTrue(owner.begin_saw_invocation_current)
            self.assertTrue(owner.begin_saw_edited_current)
            with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                         "expired_or_foreign"):
                operand.require_current()
            with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                         "source_edited_expired_or_foreign"):
                edited_lease.require_current()
            completion = executor.workspace_version_lock_mutation(
                transition, witness._frame)
            updated = edited._complete_lock_transition(transition, completion)
            self.assertEqual(executor.mutation_calls, 1)
            self.assertEqual(executor.completion_checks, 1)
            self.assertEqual(owner.complete_calls, 1)
            self.assertEqual(owner.acquire_calls, 1)
            with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                         "source_lock_transition_expired_or_foreign"):
                transition.require_current()
            updated.require_current()
            witness.require_current()
            self.assertIs(updated._original, witness)
            self.assertIs(updated._entry._acquired, owner.successor)
            self.assertEqual(updated._manifest, edited_manifest)
            self.assertEqual(len(registry._snapshots), 2)
            self.assertEqual(executor.manifest_calls, 1)
            sdk.close()

    def test_transition_slot_or_pending_drift_calls_no_writer(self):
        for drift in ("pending", "before", "expired"):
            with self.subTest(drift=drift), self._transition_fixture() as values:
                owner, _current, _registry, sdk, executor, witness, _edited, mutation, *_ = values
                transition = edited._begin_lock_transition(mutation.source_operand())
                if drift == "pending":
                    object.__setattr__(transition, "_pending", object())
                elif drift == "before":
                    object.__setattr__(transition, "_before", b"forged")
                else:
                    transition._pending.live = False
                completion = FixtureCompletion(executor, transition, witness._frame)
                with self.assertRaises(NativeSourceSemanticsUnavailable):
                    edited._complete_lock_transition(transition, completion)
                self.assertEqual(executor.completion_checks, 0)
                self.assertEqual(executor.mutation_calls, 0)
                self.assertEqual(owner.complete_calls, 0)
                if id(sdk) in semantics_sdk._SDKS:
                    sdk.close()


if __name__ == "__main__":
    unittest.main()
