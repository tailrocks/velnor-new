"""Native OriginalFS witness tests with synthetic issuer/runtime seams only."""
import copy
import hashlib
import json
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
import source_semantics_edited as edited
import source_semantics_leases as leases
import source_semantics_lifetime as lifetime
import source_semantics_manifest_read as manifest_read
import source_semantics_sdk as semantics_sdk
from source_archive_inventory_common import InventoryError
from source_intent_native_semantics_validation import NativeSourceSemanticsUnavailable
from source_semantics_leases_test import (
    FixtureAcquired, FixtureExecutor, FixtureOwner, FixtureGoverning,
    _change_file_mtime, _raw_manifest, _replace_file_same_bytes,
    _replace_root_same_bytes, _source_tree,
)

class NativeManifestFrame:
    """Synthetic native handle; the executor registry is its authority."""

    __slots__ = ("response_bytes", "invocation", "live")

    def __init__(self, response_bytes, invocation):
        self.response_bytes = response_bytes
        self.invocation = invocation
        self.live = True

def _context(root, manifest):
    lock = {"format": 1, "workspaceManifest": manifest,
            "governingLockfile": None, "lockfileBytesBase64": None}
    return {"requested_manifest": manifest, "workspace_root": str(root),
            "member_manifests": [manifest], "governingLockContext": lock,
            "governing_lockfile_sha256": None}

def _response(invocation):
    state = invocation._snapshot._state
    root = state.root
    manifest = invocation._manifest
    context = _context(root, manifest)
    result = {
        "format": 1,
        "operation": "manifest_read",
        "metadata": {"source_manifest_sha256": hashlib.sha256(
            state.manifest_bytes).hexdigest()},
        "native_context": {"before": context, "after": context},
        "compiler_context": {"kind": "not_invoked"},
        "observations": [],
    }
    return json.dumps(result, sort_keys=True, separators=(",", ":")).encode("ascii")

class NativeManifestExecutor(FixtureExecutor):
    """A fake native owner that authenticates live frame identity, not bytes."""

    def __init__(self, raw_factory=None, during=None):
        super().__init__()
        self.raw_factory = raw_factory
        self.during = during
        self.frames = {}
        self.manifest_calls = 0
        self.compiler_calls = 0

    def manifest_read(self, invocation):
        self.require_current()
        invocation.require_current()
        self.manifest_calls += 1
        raw = self.raw_factory(invocation) if self.raw_factory else _response(invocation)
        frame = NativeManifestFrame(raw, invocation)
        self.frames[id(frame)] = (frame, invocation)
        if self.during:
            self.during(invocation)
        return frame

    def require_manifest_read_frame(self, frame, invocation):
        self.require_current()
        record = self.frames.get(id(frame))
        if (type(frame) is not NativeManifestFrame or record != (frame, invocation)
                or not frame.live):
            raise NativeSourceSemanticsUnavailable("fixture_foreign_manifest_frame")

    def release_manifest_read_frame(self, frame):
        record = self.frames.pop(id(frame), None)
        if record is not None and record[0] is frame:
            frame.live = False

    def close(self):
        for frame, _invocation in self.frames.values():
            frame.live = False
        self.frames.clear()
        self.live = False

class EditedAcquired(FixtureAcquired):
    def __init__(self, root, manifest, original_manifest):
        super().__init__(root, (manifest,))
        self._original_manifest = original_manifest
        self._derived = manifest

    def derived_manifest_path(self, original_manifest):
        self.require_current()
        if original_manifest != self._original_manifest:
            raise NativeSourceSemanticsUnavailable("fixture_foreign_original_manifest")
        return self._derived

class NativeOwner(FixtureOwner):
    def require_original_snapshot(self, acquired):
        self.require_acquired_snapshot(acquired)

    def close(self):
        self.executor.close()
        super().close()

class EditedOwner(NativeOwner):
    def __init__(self, sdk, executor, original, derived):
        super().__init__(sdk, executor, (original,))
        self.original_acquired = original
        self.derived_acquired = derived
        self.original_frame = None

    def require_acquired_snapshot(self, acquired):
        self.require_current()
        if acquired not in (self.original_acquired, self.derived_acquired):
            raise NativeSourceSemanticsUnavailable("fixture_foreign_acquisition")
        acquired.require_current()

    def acquire_edited_snapshot(self, original, frame):
        self.require_current()
        if original is not self.original_acquired or frame is not self.original_frame:
            raise NativeSourceSemanticsUnavailable("fixture_foreign_edit_inputs")
        return self.derived_acquired

    def require_edited_snapshot(self, acquired, original, frame):
        self.require_current()
        if (acquired is not self.derived_acquired or original is not self.original_acquired
                or frame is not self.original_frame):
            raise NativeSourceSemanticsUnavailable("fixture_foreign_edit_snapshot")

def _fixture_sdk(lifetime, registry):
    """Synthetic owner seam: object-new SDK plus its private live registry."""
    sdk = object.__new__(semantics_sdk.SourceSemanticsSdk)
    object.__setattr__(sdk, "_lifetime", lifetime)
    object.__setattr__(sdk, "_registry", registry)
    semantics_sdk._SDKS[id(sdk)] = (sdk, lifetime, registry)
    sdk.require_current()
    return sdk

def _reset_runtime():
    semantics_sdk._SDKS.clear()
    semantics_sdk._BOUND_SDKS.clear()
    manifest_read._WITNESSES.clear()
    manifest_read._WITNESS_RECORDS.clear()
    edited._EDITED.clear()
    edited._TRANSITIONS.clear()
    leases._REGISTRIES.clear()
    leases._ENTRIES.clear()
    leases._INVOCATIONS.clear()
    leases._USED_LIFETIMES.clear()
    lifetime._LIVE_REGISTRY.clear()
    lifetime._LIVE_LIFETIME = None


class SourceSemanticsWitnessTests(unittest.TestCase):
    def tearDown(self):
        _reset_runtime()

    @contextmanager
    def _configured(self, acquisitions, executor=None, owner_type=NativeOwner):
        executor = executor or NativeManifestExecutor()
        owner = owner_type(_fixture_sdk_placeholder(), executor, *acquisitions) \
            if owner_type is EditedOwner else owner_type(
                _fixture_sdk_placeholder(), executor, acquisitions)
        stack = ExitStack()
        stack.enter_context(patch.object(cold_sdk.ColdSourceIntentSdk,
                                          "require_current", return_value=None))
        bindings = (
            (lifetime, {"_COMPILED_SOURCE_SEMANTICS_OWNER_TYPE": type(owner),
                        "_COMPILED_SOURCE_SEMANTICS_OWNER_GETTER": lambda: owner,
                        "_COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE": type(executor)}),
            (leases, {"_COMPILED_PREPARATION_OWNER_TYPE": type(owner),
                      "_COMPILED_PREPARATION_OWNER_GETTER": lambda: owner,
                      "_COMPILED_ACQUIRED_SNAPSHOT_TYPE": type(acquisitions[0])}),
            (manifest_read, {"_COMPILED_NATIVE_MANIFEST_FRAME_TYPE": NativeManifestFrame}),
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
            sdk = _fixture_sdk(current, registry)
            yield owner, current, registry, sdk, executor
        finally:
            if sdk is not None and id(sdk) in semantics_sdk._SDKS:
                sdk.close()
            elif registry is not None and id(registry) in leases._REGISTRIES:
                registry.close()
            if current is not None and id(current) in lifetime._LIVE_REGISTRY:
                current.close()
            stack.close()
            _reset_runtime()

    def test_private_constructor_and_unbound_caps_stay_closed(self):
        for constructor in (semantics_sdk.SourceSemanticsSdk,
                            manifest_read.ManifestReadWitness):
            with self.subTest(constructor=constructor.__name__), self.assertRaises(
                    NativeSourceSemanticsUnavailable):
                constructor()
        self.assertIsNone(manifest_read._COMPILED_NATIVE_MANIFEST_FRAME_TYPE)
        self.assertIsNone(semantics_sdk._COMPILED_NATIVE_LOCK_COMPLETION_TYPE)
        self.assertIsNone(edited._COMPILED_NATIVE_LOCK_TRANSITION_TYPE)

    def test_native_capture_uses_exact_frame_and_real_original_manifest(self):
        with tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            with self._configured((acquired,)) as (owner, _current, registry, sdk, executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                operand = operation.source_operand()
                result = operation.manifest_read(operand)
                witness = registry._snapshots[0]._witnesses[manifest]
                frame = witness._frame
                self.assertIs(type(frame), NativeManifestFrame)
                self.assertIs(frame.invocation, operand)
                self.assertEqual(result["operation"], "manifest_read")
                self.assertEqual(witness.observation_bytes(), frame.response_bytes)
                self.assertEqual(registry._snapshots[0]._state.manifest_bytes,
                                 _raw_manifest(root))
                self.assertEqual(executor.manifest_calls, 1)
                self.assertEqual(executor.compiler_calls, 0)
                operation.manifest_read(operand)
                result["metadata"]["tampered"] = True
                self.assertNotIn("tampered", operation.manifest_read(operand)["metadata"])
                self.assertEqual(executor.manifest_calls, 1)
                self.assertIs(owner.executor, executor)

    def test_json_frame_dict_copy_and_foreign_same_bytes_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
            root = _source_tree(directory)
            foreign_root = _source_tree(directory, "foreign")
            manifest = str(root / "Cargo.toml")
            foreign_manifest = str(foreign_root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            foreign_acquired = FixtureAcquired(foreign_root, (foreign_manifest,))
            with self._configured((acquired, foreign_acquired)) as (
                    _owner, current, registry, sdk, executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                operand = operation.source_operand()
                foreign_operand = sdk._bind_native_request(foreign_manifest,
                                                            "manifest_read").source_operand()
                operation.manifest_read(operand)
                witness = registry._snapshots[0]._witnesses[manifest]
                frame = witness._frame
                payload = json.loads(witness.observation_bytes())
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "frame"):
                    executor.require_manifest_read_frame(payload, operand)
                copied = object.__new__(NativeManifestFrame)
                copied.response_bytes, copied.invocation, copied.live = (
                    frame.response_bytes, operand, True)
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "frame"):
                    executor.require_manifest_read_frame(copied, operand)
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "frame"):
                    executor.require_manifest_read_frame(frame, foreign_operand)
                with self.assertRaises(NativeSourceSemanticsUnavailable):
                    copy.copy(witness)
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "held_binding"):
                    foreign_lifetime = object.__new__(type(current))
                    clone = object.__new__(type(witness))
                    for name, value in (("_invocation", operand), ("_lifetime", foreign_lifetime),
                                        ("_executor", executor), ("_frame", frame),
                                        ("_frame_type", NativeManifestFrame),
                                        ("_response", frame.response_bytes)):
                        object.__setattr__(clone, name, value)
                    manifest_read._WITNESSES[id(clone)] = clone
                    registry._snapshots[0]._witnesses[manifest] = clone
                    clone.require_current()
                registry._snapshots[0]._witnesses[manifest] = witness

    def test_original_mutation_before_and_during_read_stops_and_closes_sdk(self):
        for mutate in (_replace_file_same_bytes, _replace_root_same_bytes, _change_file_mtime):
            with self.subTest(phase="before", mutation=mutate.__name__), \
                    tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
                root = _source_tree(directory)
                manifest = str(root / "Cargo.toml")
                executor = NativeManifestExecutor()
                with self._configured((FixtureAcquired(root, (manifest,)),), executor) as (
                        owner, _current, _registry, sdk, _executor):
                    operation = sdk._bind_native_request(manifest, "manifest_read")
                    operand = operation.source_operand()
                    mutate(root)
                    with self.assertRaises(InventoryError):
                        operation.manifest_read(operand)
                    self.assertNotIn(id(sdk), semantics_sdk._SDKS)
                    self.assertFalse(owner.live)
                    self.assertFalse(executor.live)
            with self.subTest(phase="during", mutation=mutate.__name__), \
                    tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
                root = _source_tree(directory)
                manifest = str(root / "Cargo.toml")

                def during(_invocation):
                    mutate(root)

                executor = NativeManifestExecutor(during=during)
                with self._configured((FixtureAcquired(root, (manifest,)),), executor) as (
                        owner, _current, _registry, sdk, _executor):
                    operation = sdk._bind_native_request(manifest, "manifest_read")
                    with self.assertRaises(InventoryError):
                        operation.manifest_read(operation.source_operand())
                    self.assertNotIn(id(sdk), semantics_sdk._SDKS)
                    self.assertFalse(owner.live)
                    self.assertFalse(executor.live)

    def test_duplicate_json_and_tampered_frame_are_denied(self):
        duplicate = b'{"format":1,"format":1}'
        with tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            executor = NativeManifestExecutor(raw_factory=lambda _invocation: duplicate)
            with self._configured((FixtureAcquired(root, (manifest,)),), executor) as (
                    owner, _current, _registry, sdk, _executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "duplicate"):
                    operation.manifest_read(operation.source_operand())
                self.assertFalse(owner.live)
        with tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            with self._configured((FixtureAcquired(root, (manifest,)),)) as (
                    owner, _current, registry, sdk, executor):
                operation = sdk._bind_native_request(manifest, "manifest_read")
                operand = operation.source_operand()
                operation.manifest_read(operand)
                witness = registry._snapshots[0]._witnesses[manifest]
                frame = witness._frame
                replacement = object.__new__(NativeManifestFrame)
                replacement.response_bytes, replacement.invocation, replacement.live = (
                    frame.response_bytes, operand, True)
                object.__setattr__(witness, "_frame", replacement)
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "held_binding"):
                    witness.require_current()
                object.__setattr__(witness, "_frame", frame)
                frame.response_bytes = b"tampered"
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable, "observation"):
                    operation.manifest_read(operand)
                self.assertFalse(owner.live)
                self.assertFalse(executor.live)

    def test_original_frame_survives_native_edited_snapshot_creation(self):
        with tempfile.TemporaryDirectory(prefix="velnor-witness-") as directory:
            original_root = _source_tree(directory, "original")
            edited_root = _source_tree(directory, "edited")
            original_manifest = str(original_root / "Cargo.toml")
            edited_manifest = str(edited_root / "Cargo.toml")
            original = EditedAcquired(original_root, original_manifest, original_manifest)
            derived = EditedAcquired(edited_root, edited_manifest, original_manifest)
            executor = NativeManifestExecutor()
            owner = EditedOwner(_fixture_sdk_placeholder(), executor, original, derived)
            owner_factory = lambda _sdk, _executor, _items: owner
            with self._configured((original,), executor, owner_type=owner_factory) as (
                    _ignored, _current, registry, sdk, _executor):
                operation = sdk._bind_native_request(original_manifest, "manifest_read")
                operand = operation.source_operand()
                operation.manifest_read(operand)
                witness = registry._snapshots[0]._witnesses[original_manifest]
                owner.original_frame = witness._frame
                sealed = sdk._admit_native_edited_scratch(witness)
                self.assertIs(sealed._original, witness)
                self.assertIs(sealed._original._frame, owner.original_frame)
                self.assertEqual(sealed._original.observation_bytes(),
                                 owner.original_frame.response_bytes)
                self.assertEqual(executor.manifest_calls, 1)


def _fixture_sdk_placeholder():
    """The cold SDK object is patched only for this synthetic owner seam."""
    return object.__new__(cold_sdk.ColdSourceIntentSdk)


if __name__ == "__main__":
    unittest.main()
