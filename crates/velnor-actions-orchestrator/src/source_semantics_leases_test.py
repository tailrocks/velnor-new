"""Lease/registry negatives over real OriginalFS state.

FixtureOwner is a synthetic private issuer. It proves object identity and
lifecycle wiring only; it is not Foundation or host qualification evidence.
Roots, descriptors, manifests and mutations use the actual OriginalFS engine.
"""
import copy
import os
import stat
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
import source_semantics_leases as leases
import source_semantics_lifetime as lifetime
from source_archive_inventory_common import InventoryError
from source_archive_inventory_fs import root_descriptor
from source_archive_inventory_original import _original_inventory
from source_intent_cold_common import ColdSourceIntent
from source_intent_native_semantics_validation import NativeSourceSemanticsUnavailable
class FixtureExecutor:
    def __init__(self):
        self.live = True
    def require_current(self):
        if not self.live:
            raise NativeSourceSemanticsUnavailable("fixture_executor_closed")
class FixtureFrame:
    def __init__(self, name):
        self.name = name
        self.live = True
    def require_current(self):
        if not self.live:
            raise NativeSourceSemanticsUnavailable("fixture_original_witness_expired")
    def close(self):
        self.live = False
class FixtureGoverning:
    pass
class FixtureAcquired:
    def __init__(self, root, manifest_paths):
        self._root = str(root)
        self._manifest_paths = tuple(manifest_paths)
        self._identities = {path: object() for path in self._manifest_paths}
        self.live = True
    def require_current(self):
        if not self.live:
            raise NativeSourceSemanticsUnavailable("fixture_acquisition_closed")
    @property
    def root(self):
        self.require_current()
        return self._root
    @property
    def manifest_paths(self):
        self.require_current()
        return self._manifest_paths
    @property
    def root_descriptor(self):
        self.require_current()
        return root_descriptor(self._root)
    def require_manifest(self, manifest):
        self.require_current()
        return self._identities[manifest]

class FixtureOwner:
    compiled_role = "SourceSemantics"

    def __init__(self, sdk, executor, acquisitions):
        self.sdk = sdk
        self.executor = executor
        self.acquisitions = tuple(acquisitions)
        self.lifetime = None
        self.original = None
        self.governing = None
        self.operations = []
        self.acquire_calls = 0
        self.live = True

    def require_current(self):
        if not self.live:
            raise NativeSourceSemanticsUnavailable("fixture_owner_closed")

    def compiler_sdk(self):
        self.require_current()
        return self.sdk

    def native_executor(self):
        self.require_current()
        return self.executor

    def require_source_sdk_lifetime(self, value):
        self.require_current()
        if value is not self.lifetime:
            raise NativeSourceSemanticsUnavailable("fixture_foreign_lifetime")

    def require_acquired_snapshot(self, acquired):
        self.require_current()
        if not any(item is acquired for item in self.acquisitions):
            raise NativeSourceSemanticsUnavailable("fixture_foreign_acquisition")
        acquired.require_current()

    def acquire_source_snapshots(self):
        self.require_current()
        self.acquire_calls += 1
        return self.acquisitions

    def require_operation(self, acquired, operation, governing, original):
        self.require_current()
        if (operation in ("manifest_read", "comparison_read") and governing is not None):
            raise NativeSourceSemanticsUnavailable("fixture_governing_forbidden")
        if (governing is not None
                and (type(governing) is not FixtureGoverning
                     or governing is not self.governing)):
            raise NativeSourceSemanticsUnavailable("fixture_foreign_governing")
        if original is not None and original is not self.original:
            raise NativeSourceSemanticsUnavailable("fixture_foreign_original_witness")
        self.operations.append((acquired, operation, governing, original))

    def require_governing_context(self, governing, _snapshot):
        self.require_current()
        if type(governing) is not FixtureGoverning or governing is not self.governing:
            raise NativeSourceSemanticsUnavailable("fixture_foreign_governing")

    def close(self):
        self.live = False


def _fixture_sdk():
    """Create exact SDK type without claiming a qualified installation."""
    return object.__new__(cold_sdk.ColdSourceIntentSdk)


def _source_tree(parent, name="source"):
    root = Path(os.path.realpath(parent)) / name
    root.mkdir()
    (root / "Cargo.toml").write_bytes(b"[package]\nname='fixture'\n")
    (root / "src").mkdir()
    (root / "src" / "lib.rs").write_bytes(b"pub fn fixture() {}\n")
    return root


def _raw_manifest(root):
    descriptor = root_descriptor(str(root))
    try:
        roots = tuple(sorted(os.listdir(descriptor)))
        return _original_inventory(str(root), roots, descriptor).canonical_bytes
    finally:
        os.close(descriptor)


def _replace_root_same_bytes(root):
    before = root.stat()
    saved = root.with_name(root.name + "-old")
    root.rename(saved)
    _source_tree(root.parent, root.name)
    root.chmod(stat.S_IMODE(before.st_mode))
    os.utime(root, ns=(before.st_atime_ns, before.st_mtime_ns))
    after = root.stat()
    assert after.st_ino != before.st_ino
    assert stat.S_IMODE(after.st_mode) == stat.S_IMODE(before.st_mode)
    assert after.st_mtime_ns == before.st_mtime_ns


def _replace_file_same_bytes(root):
    path = root / "Cargo.toml"
    before = path.stat()
    replacement = root / "Cargo.toml.replacement"
    replacement.write_bytes(path.read_bytes())
    replacement.chmod(stat.S_IMODE(before.st_mode))
    os.utime(replacement, ns=(before.st_atime_ns, before.st_mtime_ns))
    os.replace(replacement, path)
    after = path.stat()
    assert after.st_ino != before.st_ino
    assert path.read_bytes() == b"[package]\nname='fixture'\n"
    assert stat.S_IMODE(after.st_mode) == stat.S_IMODE(before.st_mode)
    assert after.st_mtime_ns == before.st_mtime_ns


def _change_file_mode(root):
    path = root / "Cargo.toml"
    path.chmod(0o600)


def _change_file_mtime(root):
    path = root / "Cargo.toml"
    info = path.stat()
    os.utime(path, ns=(info.st_atime_ns, info.st_mtime_ns + 1_000_000_000))


def _change_layout(root):
    (root / "new-source-file").write_bytes(b"layout drift")


class SourceSemanticsLeaseTests(unittest.TestCase):
    def tearDown(self):
        self._reset_runtime()

    @staticmethod
    def _reset_runtime():
        leases._REGISTRIES.clear()
        leases._ENTRIES.clear()
        leases._INVOCATIONS.clear()
        leases._USED_LIFETIMES.clear()
        lifetime._LIVE_REGISTRY.clear()
        lifetime._LIVE_LIFETIME = None

    @contextmanager
    def _configured(self, acquisitions):
        owner = FixtureOwner(_fixture_sdk(), FixtureExecutor(), acquisitions)
        stack = ExitStack()
        stack.enter_context(patch.object(cold_sdk.ColdSourceIntentSdk,
                                          "require_current", return_value=None))
        for module, values in (
                (lifetime, {"_COMPILED_SOURCE_SEMANTICS_OWNER_TYPE": type(owner),
                            "_COMPILED_SOURCE_SEMANTICS_OWNER_GETTER": lambda: owner,
                            "_COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE": type(owner.executor)}),
                (leases, {"_COMPILED_PREPARATION_OWNER_TYPE": type(owner),
                          "_COMPILED_PREPARATION_OWNER_GETTER": lambda: owner,
                          "_COMPILED_ACQUIRED_SNAPSHOT_TYPE": type(acquisitions[0])}),):
            for name, value in values.items():
                stack.enter_context(patch.object(module, name, value))
        if hasattr(leases, "_COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE"):
            stack.enter_context(patch.object(leases,
                                             "_COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE",
                                             FixtureGoverning))
        current = None
        registry = None
        try:
            current = lifetime.load_source_sdk_lifetime()
            owner.lifetime = current
            registry = leases._load_snapshot_registry(current)
            yield owner, current, registry
        finally:
            if registry is not None:
                registry.close()
            if current is not None and lifetime._LIVE_REGISTRY.get(id(current)) is not None:
                try:
                    current.close()
                except (ColdSourceIntent, NativeSourceSemanticsUnavailable):
                    pass
            stack.close()
            self._reset_runtime()
    def test_default_none_denies_before_source_acquisition(self):
        leases._REGISTRIES.clear()
        lifetime._LIVE_REGISTRY.clear()
        lifetime._LIVE_LIFETIME = None
        with patch.object(lifetime, "_COMPILED_SOURCE_SEMANTICS_OWNER_TYPE", None), \
                patch.object(lifetime, "_COMPILED_SOURCE_SEMANTICS_OWNER_GETTER", None), \
                patch.object(lifetime, "_COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE", None), \
                self.assertRaisesRegex(ColdSourceIntent, "issuer_unavailable"):
            lifetime.load_source_sdk_lifetime()
        with patch.object(leases, "_COMPILED_PREPARATION_OWNER_TYPE", None), \
                patch.object(leases, "_COMPILED_PREPARATION_OWNER_GETTER", None), \
                patch.object(leases, "_COMPILED_ACQUIRED_SNAPSHOT_TYPE", None), \
                self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                       "compiled_preparation_unavailable"):
            leases._load_snapshot_registry(None)
    def test_unique_lookup_uses_real_root_descriptor_and_has_no_path_register(self):
        with tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            with self._configured((acquired,)) as (owner, _current, registry):
                entry = registry._snapshots[0]
                self.assertEqual(entry._state.manifest_bytes, _raw_manifest(root))
                self.assertEqual(entry._state.manifest_bytes[0:1], b"{")
                invocation = registry._select(manifest, "manifest_read")
                self.assertIs(invocation._snapshot, entry)
                self.assertEqual(owner.operations[-1][1], "manifest_read")
                self.assertFalse(hasattr(registry, "register"))
                with self.assertRaisesRegex(
                        NativeSourceSemanticsUnavailable, "unadmitted_or_ambiguous"):
                    registry._select(str(root / "Cargo.lock"), "manifest_read")
    def test_dict_copy_foreign_sdk_and_lifetime_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            acquired = FixtureAcquired(root, (manifest,))
            with self._configured((acquired,)) as (owner, current, registry):
                entry = registry._snapshots[0]
                invocation = registry._select(manifest, "manifest_read")
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                             "request_shape"):
                    registry._select({}, "manifest_read")
                for value in (entry, registry, invocation):
                    with self.subTest(value=type(value).__name__), self.assertRaises(
                            NativeSourceSemanticsUnavailable):
                        copy.copy(value)
                with self.assertRaisesRegex(ColdSourceIntent, "copy"):
                    copy.copy(current)
                foreign = object.__new__(lifetime.SourceSdkLifetime)
                with self.assertRaisesRegex(ColdSourceIntent, "authority"):
                    leases._load_snapshot_registry(foreign)
                owner.sdk = _fixture_sdk()
                with self.assertRaisesRegex(ColdSourceIntent, "sdk_changed"):
                    current.require_current()
    def test_lifetime_and_registry_close_expire_all_issued_objects(self):
        with tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            with self._configured((FixtureAcquired(root, (manifest,)),)) as (
                    _owner, current, registry):
                entry = registry._snapshots[0]
                invocation = registry._select(manifest, "manifest_read")
                registry.close()
                for value, reason in ((registry, "source_registry_expired_or_foreign"),
                                      (entry, "source_snapshot_expired_or_foreign"),
                                      (invocation, "source_invocation_expired_or_foreign")):
                    with self.subTest(value=type(value).__name__), self.assertRaisesRegex(
                            NativeSourceSemanticsUnavailable, reason):
                        value.require_current()
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                             "lifetime_already_issued"):
                    leases._load_snapshot_registry(current)
                current.close()
                with self.assertRaisesRegex(ColdSourceIntent, "authority"):
                    current.require_current()
    def test_manifest_lookup_rejects_outside_and_ambiguous_entries(self):
        with tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
            left = _source_tree(directory, "left")
            right = _source_tree(directory, "right")
            left_manifest, right_manifest = (str(left / "Cargo.toml"),
                                             str(right / "Cargo.toml"))
            with self._configured((FixtureAcquired(left, (left_manifest,)),
                                   FixtureAcquired(right, (right_manifest,)))) as (
                    _owner, _current, registry):
                with self.assertRaisesRegex(
                        NativeSourceSemanticsUnavailable, "unadmitted_or_ambiguous"):
                    registry._select(str(left / "Cargo.lock"), "manifest_read")
                selected = registry._select(right_manifest, "manifest_read")
                self.assertEqual(selected._manifest, right_manifest)
        with tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            duplicate = (FixtureAcquired(root, (manifest,)),
                         FixtureAcquired(root, (manifest,)))
            with self._configured(duplicate) as (_owner, _current, registry):
                with self.assertRaisesRegex(
                        NativeSourceSemanticsUnavailable, "unadmitted_or_ambiguous"):
                    registry._select(manifest, "manifest_read")
    def test_original_witness_is_exact_and_survives_frame_replacement(self):
        with tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
            root = _source_tree(directory)
            manifest = str(root / "Cargo.toml")
            original, edited = FixtureFrame("original"), FixtureFrame("edited")
            governing = FixtureGoverning()
            with self._configured((FixtureAcquired(root, (manifest,)),)) as (
                    owner, _current, registry):
                owner.original, owner.governing = original, governing
                for operation, context in (("full_locked_metadata", {}),
                                           ("manifest_read", governing),
                                           ("comparison_read", governing)):
                    with self.subTest(operation=operation), self.assertRaises(
                            NativeSourceSemanticsUnavailable):
                        registry._select(manifest, operation, governing=context)
                invocation = registry._select(manifest, "full_locked_metadata",
                                              governing=governing, original=original)
                self.assertIs(invocation._original, original)
                owner.original = edited
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                             "foreign_original_witness"):
                    invocation.require_current()
                owner.original = original
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                             "foreign_original_witness"):
                    registry._select(manifest, "full_locked_metadata",
                                     governing=governing, original=edited)
                original.close()
                with self.assertRaisesRegex(NativeSourceSemanticsUnavailable,
                                             "original_witness_expired"):
                    invocation.require_current()
    def test_original_mutations_stop_selection_and_existing_invocation(self):
        mutations = (_replace_root_same_bytes, _replace_file_same_bytes,
                     _change_file_mode, _change_file_mtime, _change_layout)
        for mutate in mutations:
            with self.subTest(mutation=mutate.__name__), \
                    tempfile.TemporaryDirectory(prefix="velnor-source-lease-") as directory:
                root = _source_tree(directory)
                manifest = str(root / "Cargo.toml")
                with self._configured((FixtureAcquired(root, (manifest,)),)) as (
                        _owner, _current, registry):
                    invocation = registry._select(manifest, "manifest_read")
                    mutate(root)
                    actions = (lambda: registry._select(manifest, "manifest_read"),
                               invocation.require_current, invocation.source_operand)
                    for action in actions:
                        with self.subTest(action=getattr(action, "__name__", "select")), \
                                self.assertRaises(InventoryError):
                            action()


if __name__ == "__main__":
    unittest.main()
