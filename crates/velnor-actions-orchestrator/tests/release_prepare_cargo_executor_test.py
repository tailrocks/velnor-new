"""Pure observation validation and rejection; no synthetic SDK authority grant."""
import base64
import builtins
import json
import os
from pathlib import Path
import sys
import tempfile
from types import ModuleType
import unittest


DIRECTORY = Path(__file__).parent.parent / "src"
INVENTORY_DIRECTORY = DIRECTORY.parent.parent / "velnor-actions-mise" / "src"
INVENTORY_SOURCES = {name: (INVENTORY_DIRECTORY / (name + ".py")).read_text() for name in (
    "metadata_container", "opaque_inventory_metadata", "source_archive_inventory_common",
    "source_archive_inventory_fs", "source_archive_inventory_leaf", "source_archive_inventory_walk",
    "source_archive_inventory", "source_archive_inventory_original")}
INVENTORY_MODULES = {}


def observation_import(name, globals=None, locals=None, fromlist=(), level=0):
    if level == 0 and name in INVENTORY_SOURCES:
        if name not in INVENTORY_MODULES:
            module = ModuleType("_cargo_comparison_observation_" + name)
            INVENTORY_MODULES[name] = module
            sys.modules[module.__name__] = module
            module.__dict__["__builtins__"] = OBSERVATION_BUILTINS
            exec(compile(INVENTORY_SOURCES[name], name + ".py", "exec"), module.__dict__)
        return INVENTORY_MODULES[name]
    return builtins.__import__(name, globals, locals, fromlist, level)


OBSERVATION_BUILTINS = dict(vars(builtins), __import__=observation_import)
NAMESPACE = {"__builtins__": OBSERVATION_BUILTINS}
for filename in ("release_reconcile_common.py", "release_prepare_docs.py",
                 "source_intent_native_semantics_validation.py",
                 "source_intent_native_semantics.py", "release_prepare_cargo_executor.py"):
    exec(compile((DIRECTORY / filename).read_text(), filename, "exec"), NAMESPACE)
ERROR = NAMESPACE["ReconcileError"]
SDK_ERROR = NAMESPACE["NativeSourceSemanticsUnavailable"]
SDK = NAMESPACE["NativeSourceSemanticsSdk"]
CALLBACK = NAMESPACE["NativeCargoExecutorCallback"]


def observation(operation="manifest_read"):
    return {"format": 1, "operation": operation, "metadata": {"packages": []},
            "native_context": {}, "compiler_context": {}, "observations": []}


def files_observation():
    return {"format": 1, "package_name": "demo", "package_version": "1.0.0", "files": [
        {"path": "src/z.rs", "role": "source", "source_path": "src/z.rs"},
        {"path": "LICENSE", "role": "source", "source_path": "../LICENSE"},
        {"path": "Cargo.toml", "role": "normalized_manifest", "source_path": None}]}


class CargoObservationTests(unittest.TestCase):
    def metadata(self, value, operation="manifest_read"):
        return NAMESPACE["_prepare_cargo_metadata_observation"](value, operation)

    def test_metadata_consumption_is_not_authority(self):
        value = observation()
        self.assertEqual(self.metadata(value), {"packages": []})
        with self.assertRaisesRegex(ERROR, "sdk_origin"):
            CALLBACK(value)

    def test_exact_format_and_operation(self):
        for value in (True, "1", 2, None):
            with self.subTest(value=value), self.assertRaisesRegex(ERROR, "binding"):
                self.metadata(dict(observation(), format=value))
        for operation in ("identity", "ManifestRead", None, True):
            with self.subTest(operation=operation), self.assertRaisesRegex(ERROR, "operation"):
                self.metadata(observation(), operation)
        with self.assertRaisesRegex(ERROR, "binding"):
            self.metadata(observation("full_locked_metadata"))

    def test_response_envelope_and_metadata_shape(self):
        for field in observation():
            value = observation()
            del value[field]
            with self.subTest(field=field), self.assertRaisesRegex(ERROR, "fields"):
                self.metadata(value)
        with self.assertRaisesRegex(ERROR, "fields"):
            self.metadata(dict(observation(), argv=["cargo"]))
        with self.assertRaisesRegex(ERROR, "metadata"):
            self.metadata(dict(observation(), metadata=[]))

    def test_identity_is_observation_without_duplicate_pins(self):
        validate = NAMESPACE["_prepare_cargo_identity_observation"]
        value = b"cargo observed-version\ncommit-hash: observed-source-revision\n"
        self.assertIs(validate(value), value)
        for invalid in (None, True, "cargo", b"", b"x" * (64 * 1024 + 1),
                        bytearray(value), {"runtimeVersion": "cargo", "sourceRevision": "revision"}):
            with self.subTest(invalid=type(invalid)), self.assertRaisesRegex(ERROR, "bytes"):
                validate(invalid)
        self.assertEqual(validate(b"x" * (64 * 1024)), b"x" * (64 * 1024))
        with self.assertRaisesRegex(ERROR, "encoding"):
            validate(b"\xff")

    def test_governing_context_frozen_as_observation_only(self):
        value = {"format": 1, "workspaceManifest": "/source/Cargo.toml",
                 "governingLockfile": "/source/Cargo.lock",
                 "lockfileBytesBase64": base64.b64encode(b"observed lock").decode("ascii")}
        frozen, raw = NAMESPACE["_docs_freeze_lock_observation"](value)
        value["workspaceManifest"] = "/other/Cargo.toml"
        self.assertEqual(frozen["workspaceManifest"], "/source/Cargo.toml")
        self.assertEqual(raw, b"observed lock")
        with self.assertRaises(TypeError):
            frozen["format"] = 2
        with self.assertRaisesRegex(ERROR, "sdk_origin"):
            CALLBACK(frozen)

    def test_metadata_bytes_serialize_actual_native_data(self):
        value = observation()
        value["metadata"] = {"package": "nátive", "extra": [1, False, None]}
        raw = NAMESPACE["_prepare_cargo_metadata_bytes"](value, "manifest_read")
        self.assertIs(type(raw), bytes)
        self.assertEqual(json.loads(raw), value["metadata"])
        for invalid in (float("nan"), object(), "bad\ud800"):
            value["metadata"] = {"bad": invalid}
            with self.subTest(invalid=type(invalid)), self.assertRaisesRegex(ERROR, "encoding"):
                NAMESPACE["_prepare_cargo_metadata_bytes"](value, "manifest_read")

    def test_mutation_bytes_come_from_exact_native_after_context(self):
        project = NAMESPACE["_prepare_cargo_mutation_bytes"]
        value = observation("workspace_version_lock_mutation")
        governing = {"format": 1, "workspaceManifest": "/source/Cargo.toml",
                     "governingLockfile": "/source/Cargo.lock",
                     "lockfileBytesBase64": base64.b64encode(b"actual updated lock").decode("ascii")}
        value["native_context"] = {"before": {}, "after": {"governingLockContext": governing}}
        self.assertEqual(project(value), b"actual updated lock")
        for invalid in (None, [], {}, {"after": None}, {"after": {}},
                        {"after": {"governingLockContext": {}}}):
            with self.subTest(context=invalid), self.assertRaises(ERROR):
                project(dict(value, native_context=invalid))
        governing["lockfileBytesBase64"] = "invalid"
        with self.assertRaisesRegex(ERROR, "governing_bytes"):
            project(value)


class CargoAuthorityRejectionTests(unittest.TestCase):
    def test_dict_duck_and_subclass_rejected_without_calls(self):
        class Duck:
            def require_current(self):
                raise AssertionError("duck must not be called")
        class Derived(SDK):
            pass
        for candidate in ({}, Duck(), object.__new__(Derived), None):
            with self.subTest(candidate=type(candidate)), self.assertRaisesRegex(ERROR, "sdk_origin"):
                CALLBACK(candidate)

    def test_exact_unsealed_sdk_rejected(self):
        with self.assertRaisesRegex(SDK_ERROR, "sdk_authority"):
            CALLBACK(object.__new__(SDK))

    def test_actual_loader_denies_missing_issuance(self):
        with self.assertRaisesRegex(SDK_ERROR, "unavailable"):
            NAMESPACE["load_native_source_semantics_sdk"]()


class CargoComparisonObservationTests(unittest.TestCase):
    def project(self, response):
        return NAMESPACE["_prepare_cargo_package_files_observation"](response)

    def test_native_order_paths_and_parent_source_observation_preserved(self):
        value = files_observation()
        self.assertEqual(self.project(value), ["src/z.rs", "LICENSE", "Cargo.toml"])
        self.assertEqual(value["files"][1]["source_path"], "../LICENSE")
        with self.assertRaisesRegex(ERROR, "sdk_origin"):
            CALLBACK(value)

    def test_archive_path_substitution_rejected(self):
        for path in ("", "/absolute", "../LICENSE", "src/../file", "src/./file",
                     "src//file", "src/", "bad\x00", "bad\ud800"):
            value = files_observation()
            value["files"][0]["path"] = path
            with self.subTest(path=repr(path)), self.assertRaises(ERROR):
                self.project(value)
        value = files_observation()
        value["files"].append(dict(value["files"][0]))
        with self.assertRaisesRegex(ERROR, "duplicate"):
            self.project(value)

    def test_closed_envelope_and_records(self):
        value = files_observation()
        for field in value:
            missing = dict(value)
            del missing[field]
            with self.subTest(field=field), self.assertRaisesRegex(ERROR, "fields"):
                self.project(missing)
        for invalid in (True, "1", 2):
            with self.assertRaisesRegex(ERROR, "shape"):
                self.project(dict(value, format=invalid))
        with self.assertRaisesRegex(ERROR, "fields"):
            self.project(dict(value, manifest_path="/caller/Cargo.toml"))
        for record in ({"path": "file", "role": "source"},
                       {"path": "file", "role": "unknown", "source_path": None},
                       {"path": "file", "role": "source", "source_path": True},
                       {"path": "file", "role": "source", "source_path": None},
                       {"path": "Cargo.toml.orig", "role": "original_manifest", "source_path": None}):
            with self.subTest(record=record), self.assertRaises(ERROR):
                self.project(dict(value, files=[record]))


class NativeComparisonSnapshotTests(unittest.TestCase):
    """Read-only fixture observations passed only to the private validator."""
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.package = self.root / "package"
        self.package.mkdir()
        (self.package / "Cargo.toml").write_bytes(b"observed manifest")
        (self.package / "src").mkdir()
        (self.package / "src" / "lib.rs").write_bytes(b"observed source")
        (self.root / "LICENSE").write_bytes(b"shared license")
        (self.package / "file-link").symlink_to("src/lib.rs")
        (self.package / "directory-alias").symlink_to("src", target_is_directory=True)
        self.request = {"manifest_path": str(self.package / "Cargo.toml"),
                        "cargo_home": str(self.root / "cargo-home"), "package_name": "demo"}
        self.snapshot = self.capture_snapshot()

    def capture_snapshot(self):
        fs = observation_import("source_archive_inventory_fs")
        original = observation_import("source_archive_inventory_original")
        descriptor = fs.root_descriptor(str(self.root))
        try:
            return original._original_inventory(str(self.root),
                tuple(sorted(os.listdir(descriptor))), descriptor).canonical_bytes
        finally:
            os.close(descriptor)

    def validate(self, source):
        result = {"format": 1, "package_name": "demo", "package_version": "1.0.0",
                  "files": [{"path": "archive-file", "role": "source", "source_path": source}]}
        NAMESPACE["_validate_comparison"](result, self.request, str(self.root), self.snapshot)

    def test_regular_internal_links_and_shared_parent_accepted(self):
        for source in ("src/lib.rs", "file-link", "directory-alias/lib.rs", "../LICENSE"):
            with self.subTest(source=source):
                self.validate(source)

    def test_authentic_source_modes_preserved(self):
        (self.package / "src" / "lib.rs").chmod(0o775)
        self.snapshot = self.capture_snapshot()
        for source in ("src/lib.rs", "file-link", "directory-alias/lib.rs"):
            with self.subTest(source=source):
                self.validate(source)

    def test_escaping_source_rejected(self):
        with self.assertRaises(SDK_ERROR):
            self.validate("../../outside")
        (self.package / "file-link").unlink()
        (self.package / "file-link").symlink_to("../../outside")
        with self.assertRaises(SDK_ERROR):
            self.validate("file-link")

    def test_outside_link_returning_into_snapshot_rejected(self):
        outside = tempfile.TemporaryDirectory()
        self.addCleanup(outside.cleanup)
        link = Path(outside.name).resolve() / "link"
        link.symlink_to(self.package / "src" / "lib.rs")
        source = os.path.relpath(link, self.package)
        self.assertTrue(source.startswith("../"))
        self.assertEqual(link.resolve(), self.package / "src" / "lib.rs")
        with self.assertRaises(SDK_ERROR):
            self.validate(source)

    def test_internal_link_escape_then_return_rejected(self):
        outside = tempfile.TemporaryDirectory()
        self.addCleanup(outside.cleanup)
        back = Path(outside.name).resolve() / "back"
        target = self.package / "src" / "lib.rs"
        back.symlink_to(target)
        internal = self.package / "file-link"
        internal.unlink()
        internal.symlink_to(os.path.relpath(back, self.package))
        self.assertTrue(internal.readlink().as_posix().startswith("../"))
        self.assertEqual(internal.resolve(), target)
        with self.assertRaises(SDK_ERROR):
            self.validate("file-link")

    def test_link_substitution_preserves_parent_traversal_boundary(self):
        parent = self.root
        self.root = parent / "route-root"
        self.package = self.root / "package"
        (self.package / "src").mkdir(parents=True)
        (self.root / "dir").mkdir()
        outside = parent / "outside"
        outside.mkdir()
        (self.package / "Cargo.toml").write_bytes(b"observed manifest")
        target = self.package / "src" / "lib.rs"
        target.write_bytes(b"observed source")
        (self.root / "LICENSE").write_bytes(b"shared license")
        (self.package / "alias").symlink_to("../dir", target_is_directory=True)
        (outside / "back").symlink_to(target)
        self.request = {"manifest_path": str(self.package / "Cargo.toml"),
                        "cargo_home": str(parent / "cargo-home"), "package_name": "demo"}
        self.snapshot = self.capture_snapshot()
        source = "alias/../../outside/back"
        lexical = os.path.normpath(str(self.package / source))
        self.assertEqual(os.path.commonpath((str(self.root), lexical)), str(self.root))
        self.assertEqual((self.package / source).resolve(), target)
        self.assertEqual((self.package / "alias/../LICENSE").resolve(), self.root / "LICENSE")
        self.validate("alias/../LICENSE")
        with self.assertRaises(SDK_ERROR):
            self.validate(source)

    def test_safe_depth_changing_alias_parent_route_accepted(self):
        (self.root / "dir" / "deep" / "deeper").mkdir(parents=True)
        (self.package / "deep-alias").symlink_to("../dir/deep/deeper", target_is_directory=True)
        self.snapshot = self.capture_snapshot()
        source = "deep-alias/../../../LICENSE"
        lexical = os.path.normpath(str(self.package / source))
        self.assertNotEqual(os.path.commonpath((str(self.root), lexical)), str(self.root))
        self.assertEqual((self.package / source).resolve(), self.root / "LICENSE")
        self.validate(source)

    def test_retargeted_file_and_directory_links_rejected(self):
        for name, target in (("file-link", "../LICENSE"), ("directory-alias", "..")):
            path = self.package / name
            path.unlink()
            path.symlink_to(target)
            with self.subTest(name=name), self.assertRaises(SDK_ERROR):
                self.validate(name if name == "file-link" else name + "/LICENSE")

    def test_target_bytes_changed_rejected(self):
        (self.package / "src" / "lib.rs").write_bytes(b"mutated target")
        for source in ("src/lib.rs", "file-link", "directory-alias/lib.rs"):
            with self.subTest(source=source), self.assertRaises(SDK_ERROR):
                self.validate(source)


if __name__ == "__main__":
    unittest.main()
