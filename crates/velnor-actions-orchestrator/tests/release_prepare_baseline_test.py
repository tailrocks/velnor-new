"""Pure baseline byte/mode/layout observations; no Cargo, Git or network."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile
import unittest
from unittest.mock import patch


DIRECTORY = Path(__file__).parent.parent / "src"
NAMESPACE = {"REGISTRY_FILE_LIMIT": 32 * 1024 * 1024,
             "REGISTRY_TAR_LIMIT": 128 * 1024 * 1024, "REGISTRY_MEMBER_LIMIT": 10000}
for filename in ("release_reconcile_common.py", "release_prepare_registry.py",
                 "release_prepare_baseline.py"):
    exec(compile((DIRECTORY / filename).read_text(), filename, "exec"), NAMESPACE)
ERROR = NAMESPACE["ReconcileError"]
sys.path.insert(0, str(DIRECTORY))
sys.path.insert(0, str(DIRECTORY.parents[1] / "velnor-actions-mise" / "src"))
from source_intent_cold_sdk import ColdSourceIntentSdk, ColdSourceIntentInstalledTool
from source_intent_cold_common import ColdSourceIntent
NAMESPACE.update(ColdSourceIntentSdk=ColdSourceIntentSdk,
                 ColdSourceIntentInstalledTool=ColdSourceIntentInstalledTool)


def copy_snapshot(snapshot, destination):
    """Mock native archive extraction/copy; never used by SDK product code."""
    destination.mkdir(mode=0o700)
    for relative, (_, raw) in sorted(snapshot.items(), key=lambda item: (
            len(Path(item[0]).parts), item[0])):
        if not relative:
            continue
        path = destination / relative
        if raw is None:
            path.mkdir(mode=0o700)
        else:
            path.write_bytes(raw)
    for relative, (mode, _) in sorted(snapshot.items(), reverse=True):
        (destination / relative).chmod(mode)
    return destination


class BaselineSourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / "original"
        self.source.mkdir()
        (self.source / "Cargo.toml").write_bytes(b"original manifest\x00bytes")
        (self.source / "empty").mkdir()
        (self.source / "empty").chmod(0o750)
        (self.source / "source.rs").write_bytes(b"fn untouched() {}\n")
        (self.source / "source.rs").chmod(0o640)
        self.snapshot = NAMESPACE["_baseline_tree"](self.source)
        self.derived = self.root / "derived"

    def copy(self):
        return copy_snapshot(self.snapshot, self.derived)

    def test_copy_preserves_exact_bytes_full_modes_and_empty_layout(self):
        self.assertEqual(self.copy(), self.derived)
        self.assertEqual(NAMESPACE["_baseline_tree"](self.derived), self.snapshot)
        self.assertEqual(NAMESPACE["_baseline_tree"](self.source), self.snapshot)

    def test_only_governing_lock_addition_permitted(self):
        self.copy()
        (self.derived / "Cargo.lock").write_bytes(b"native generated lock")
        lock = NAMESPACE["_baseline_file"](self.derived / "Cargo.lock")
        NAMESPACE["_baseline_derived_unchanged"](
            self.derived, self.snapshot, "Cargo.lock", lock)
        (self.derived / "Cargo.lock").write_bytes(b"changed lock")
        with self.assertRaisesRegex(ERROR, "derived_lock_changed"):
            NAMESPACE["_baseline_derived_unchanged"](
                self.derived, self.snapshot, "Cargo.lock", lock)

    def test_source_mutations_include_nonexecutable_modes_and_directories(self):
        for mutation in (lambda: (self.source / "source.rs").chmod(0o600),
                         lambda: (self.source / "empty").chmod(0o700),
                         lambda: (self.source / "new-empty").mkdir(),
                         lambda: (self.source / "Cargo.toml").write_bytes(b"normalized")):
            with self.subTest(mutation=mutation):
                mutation()
                with self.assertRaisesRegex(ERROR, "input_tree_changed"):
                    NAMESPACE["_baseline_unchanged"]({self.source: self.snapshot}, {})
                self.snapshot = NAMESPACE["_baseline_tree"](self.source)

    def test_symlink_and_special_files_rejected(self):
        (self.source / "link").symlink_to(self.source / "source.rs")
        with self.assertRaisesRegex(ERROR, "source_type"):
            NAMESPACE["_baseline_tree"](self.source)
        with self.assertRaisesRegex(ERROR, "input_symlink"):
            NAMESPACE["_baseline_file"](self.source / "link")

    def test_walk_error_fails_closed(self):
        def walk(*args, **kwargs):
            kwargs["onerror"](PermissionError("unreadable"))
        with patch.object(NAMESPACE["os"], "walk", walk):
            with self.assertRaisesRegex(ERROR, "source_walk"):
                NAMESPACE["_baseline_tree"](self.source)

    def test_archive_mutation_or_chmod_detected(self):
        archive = self.root / "original.crate"
        archive.write_bytes(b"authenticated archive")
        archive.chmod(0o644)
        snapshot = NAMESPACE["_baseline_file"](archive)
        archive.chmod(0o600)
        with self.assertRaisesRegex(ERROR, "input_file_changed"):
            NAMESPACE["_baseline_unchanged"]({}, {archive: snapshot})
        archive.chmod(snapshot[0])
        archive.write_bytes(b"changed archive")
        with self.assertRaisesRegex(ERROR, "input_file_changed"):
            NAMESPACE["_baseline_unchanged"]({}, {archive: snapshot})

    def test_manifest_or_git_addition_rejected(self):
        self.copy()
        (self.derived / ".git").mkdir()
        with self.assertRaisesRegex(ERROR, "derived_source_changed"):
            NAMESPACE["_baseline_derived_unchanged"](
                self.derived, self.snapshot, "Cargo.lock")

    def test_native_context_lock_can_be_nested_without_root_search(self):
        self.copy()
        (self.derived / "empty" / "Cargo.lock").write_bytes(b"native nested lock")
        lock = NAMESPACE["_baseline_file"](self.derived / "empty" / "Cargo.lock")
        NAMESPACE["_baseline_derived_unchanged"](
            self.derived, self.snapshot, "empty/Cargo.lock", lock)
        with self.assertRaisesRegex(ERROR, "derived_source_changed"):
            NAMESPACE["_baseline_derived_unchanged"](
                self.derived, self.snapshot, "Cargo.lock")

    def test_unrelated_file_and_directory_additions_rejected(self):
        self.copy()
        (self.derived / "Cargo.lock").write_bytes(b"native lock")
        (self.derived / "Cargo.toml.orig").write_bytes(b"synthetic manifest")
        with self.assertRaisesRegex(ERROR, "derived_source_changed"):
            NAMESPACE["_baseline_derived_unchanged"](
                self.derived, self.snapshot, "Cargo.lock")


class BaselineResolutionTests(unittest.TestCase):
    def setUp(self):
        BaselineSourceTests.setUp(self)
        self.archive = self.root / "original.crate"
        self.archive.write_bytes(b"authenticated native archive fixture")
        self.index = self.root / "index-version.json"
        self.index.write_bytes(b"authenticated native index fixture")
        self.registry = self.root / "registry"
        self.registry.mkdir()
        (self.registry / "index").mkdir()
        (self.registry / "index" / "de").mkdir()
        (self.registry / "index" / "de" / "mo").mkdir()
        self.registry_index = self.registry / "index" / "de" / "mo" / "demo"
        self.registry_index.write_bytes(b"native index bytes")
        (self.registry / "demo-1.2.3.crate").write_bytes(self.archive.read_bytes())
        self.registries = [{"source_id": NAMESPACE["BASELINE_SOURCE_ID"],
            "root": str(self.registry), "index": {"de/mo/demo": hashlib.sha256(
                self.registry_index.read_bytes()).hexdigest()}, "archives": {
                "demo-1.2.3.crate": hashlib.sha256(self.archive.read_bytes()).hexdigest()}}]
        inventory = {path: {"sha256": hashlib.sha256(raw).hexdigest(),
            "mode": "100755" if mode & 0o111 else "100644"}
            for path, (mode, raw) in self.snapshot.items() if raw is not None}
        self.authenticated = {"name": "demo", "version": "1.2.3",
            "package_root": str(self.source), "archive_path": str(self.archive),
            "archive_sha256": hashlib.sha256(self.archive.read_bytes()).hexdigest(),
            "checksum": hashlib.sha256(self.archive.read_bytes()).hexdigest(),
            "index_version_path": str(self.index), "index_version_sha256": hashlib.sha256(
                self.index.read_bytes()).hexdigest(), "inventory_sha256": hashlib.sha256(
                json.dumps(inventory, sort_keys=True, separators=(",", ":")).encode()).hexdigest()}
        self.output = self.root / "output"
        self.output.mkdir()
        self.toolchain = self.root / "compiler-pair"
        (self.toolchain / "bin").mkdir(parents=True)
        for tool in ("cargo", "rustc"):
            (self.toolchain / "bin" / tool).write_bytes(b"mock compiler pair image")
            (self.toolchain / "bin" / tool).chmod(0o755)
        self.calls = []

    def native(self, request):
        self.calls.append(request)
        self.assertEqual(request["toolchain_root"], str(self.toolchain))
        self.assertEqual(request["operation"]["source_id"], self.registries[0]["source_id"])
        work = Path(request["work_root"])
        self.assertFalse(work.exists())
        work.mkdir(mode=0o700)
        original = work / "original" / "demo-1.2.3"
        derived = work / "source" / "demo-1.2.3"
        for tree in (original, derived):
            tree.parent.mkdir()
            copy_snapshot(self.snapshot, tree)
        lock = derived / "Cargo.lock"
        lock.write_bytes(b"native Cargo generated lock")
        context = lambda tree: {"workspace_manifest": str(tree / "Cargo.toml"),
            "governing_lockfile": str(tree / "Cargo.lock"),
            "governing_lockfile_relative": "Cargo.lock"}
        return {"format": 1, "result": {"kind": "derive_lock", "original_tree": str(original),
            "derived_tree": str(derived), "original_context": dict(context(original),
                governing_lockfile_absent=True), "derived_context": context(derived),
            "derived_lockfile": str(lock), "derived_lockfile_sha256": hashlib.sha256(
                lock.read_bytes()).hexdigest(), "package_name": "demo", "package_version": "1.2.3"},
            "observations": [{"source_id": self.registries[0]["source_id"], "kind": "index",
                "path": "de/mo/demo", "sha256": self.registries[0]["index"]["de/mo/demo"]},
                {"source_id": self.registries[0]["source_id"], "kind": "archive",
                 "path": "demo-1.2.3.crate", "sha256": self.registries[0]["archives"][
                     "demo-1.2.3.crate"]}]}

    def prepare(self, native=None):
        return NAMESPACE["_prepare_baseline_lock_observed"](self.authenticated, self.registries,
            self.output, native or self.native, self.toolchain)

    def test_native_observation_preserves_authenticated_original(self):
        result = self.prepare()
        self.assertEqual(NAMESPACE["_baseline_tree"](self.source), self.snapshot)
        self.assertEqual(Path(result["result"]["derived_lockfile"]).read_bytes(),
                         b"native Cargo generated lock")

    def test_missing_unknown_or_selfasserted_response_fails_closed(self):
        for response in (None, {}, {"format": 1, "trusted": True}):
            with self.subTest(response=response), self.assertRaises(ERROR):
                self.prepare(lambda _: response)

    def test_input_mutations_detected_even_when_native_raises(self):
        for path in (self.source / "Cargo.toml", self.archive, self.index,
                     self.registry_index):
            original = path.read_bytes()
            def fail(request):
                path.write_bytes(b"mutated")
                raise OSError("native resolver failure")
            with self.subTest(path=path), self.assertRaisesRegex(ERROR, "input_.*changed"):
                self.prepare(fail)
            path.write_bytes(original)

    def test_unknown_registry_observation_rejected(self):
        def native(request):
            result = self.native(request)
            result["observations"][0]["path"] = "unknown"
            return result
        with self.assertRaises(ERROR):
            self.prepare(native)

    def test_existing_governing_lock_cannot_be_rescued_by_absence_boolean(self):
        (self.source / "Cargo.lock").write_bytes(b"original stale lock")
        self.snapshot = NAMESPACE["_baseline_tree"](self.source)
        inventory = {path: {"sha256": hashlib.sha256(raw).hexdigest(),
            "mode": "100755" if mode & 0o111 else "100644"}
            for path, (mode, raw) in self.snapshot.items() if raw is not None}
        self.authenticated["inventory_sha256"] = hashlib.sha256(json.dumps(
            inventory, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        with self.assertRaises(ERROR):
            self.prepare()

    def test_native_manifest_rewrite_and_synthetic_git_rejected(self):
        def native(request):
            result = self.native(request)
            (Path(result["result"]["derived_tree"]) / ".git").mkdir()
            return result
        with self.assertRaises(ERROR):
            self.prepare(native)

    def test_unmodified_native_failure_preserved(self):
        def native(_):
            raise OSError("native resolver failure")
        with self.assertRaisesRegex(OSError, "native resolver failure"):
            self.prepare(native)

    def test_native_request_cannot_change_frozen_universe(self):
        def native(request):
            result = self.native(request)
            request["registries"][0]["index"]["unknown"] = None
            return result
        with self.assertRaisesRegex(ERROR, "native_request_changed"):
            self.prepare(native)

    def test_lock_digest_and_result_kind_are_closed(self):
        for key, value in (("derived_lockfile_sha256", "0" * 64), ("kind", "select_latest")):
            def native(request):
                result = self.native(request)
                result["result"][key] = value
                return result
            with self.subTest(key=key), self.assertRaises(ERROR):
                self.prepare(native)

    def test_extracted_original_checked_when_native_raises(self):
        def native(request):
            result = self.native(request)
            Path(result["result"]["derived_lockfile"]).unlink()
            (Path(result["result"]["original_tree"]) / "Cargo.toml").write_bytes(b"rewritten")
            raise OSError("native failure")
        with self.assertRaisesRegex(ERROR, "native_original_changed"):
            self.prepare(native)

    def test_derived_tree_cannot_escape_controlled_work_directory(self):
        def native(request):
            result = self.native(request)
            result["result"]["derived_tree"] = str(self.source)
            return result
        with self.assertRaises(ERROR):
            self.prepare(native)

    def test_private_work_directory_mode_cannot_change(self):
        def native(request):
            result = self.native(request)
            Path(request["work_root"]).chmod(0o755)
            return result
        with self.assertRaisesRegex(ERROR, "work_privacy_changed"):
            self.prepare(native)

    def test_registry_inventory_binds_actual_bytes_and_absence(self):
        cases = [("index", "de/mo/demo", "0" * 64),
                 ("index", "de/mo/demo", None),
                 ("archives", "missing-1.0.0.crate", "0" * 64),
                 ("archives", "nested/demo-1.2.3.crate", "0" * 64),
                 ("archives", "nested//demo-1.2.3.crate", "0" * 64),
                 ("index", "de//mo/demo", "0" * 64)]
        for kind, path, digest in cases:
            registries = json.loads(json.dumps(self.registries))
            registries[0][kind][path] = digest
            with self.subTest(kind=kind, path=path, digest=digest), self.assertRaises(
                    (ERROR, FileNotFoundError)):
                NAMESPACE["_baseline_registries"](registries)

    def test_negative_index_requires_actual_missing_path(self):
        self.registries[0]["index"]["not-published"] = None
        NAMESPACE["_baseline_registries"](self.registries)
        (self.registry / "index" / "not-published").mkdir()
        with self.assertRaisesRegex(ERROR, "negative_present"):
            NAMESPACE["_baseline_registries"](self.registries)

    def test_required_native_index_and_archive_reads_cannot_be_omitted(self):
        request = {"work_root": str(self.root / "native-test"), "toolchain_root": str(self.toolchain)}
        result = self.native(request | {"operation": {"source_id": self.registries[0]["source_id"]}})
        observations = result["observations"]
        sources = {self.registries[0]["source_id"]: self.registries[0]}
        for records in ([], observations[:1], observations[1:]):
            with self.subTest(records=records), self.assertRaisesRegex(ERROR, "missing_operation_read"):
                NAMESPACE["_baseline_observations"](records, sources, self.authenticated)
        with self.assertRaisesRegex(ERROR, "observation_duplicate"):
            NAMESPACE["_baseline_observations"](observations * 2, sources, self.authenticated)

    def test_native_context_rejects_lexical_path_normalization(self):
        context = {"workspace_manifest": str(self.source / "Cargo.toml"),
            "governing_lockfile": str(self.source / "Cargo.lock"),
            "governing_lockfile_relative": "Cargo.lock", "governing_lockfile_absent": True}
        for manifest in (str(self.source) + "//Cargo.toml",
                         str(self.source) + "/./Cargo.toml",
                         str(self.source) + "/empty/../Cargo.toml", "Cargo.toml"):
            with self.subTest(manifest=manifest), self.assertRaisesRegex(ERROR, "input_path"):
                NAMESPACE["_baseline_native_context"](
                    context | {"workspace_manifest": manifest}, self.source, True)

    def test_public_facade_rejects_unsealed_ducktyped_and_subclass_sdks(self):
        class Subclass(ColdSourceIntentSdk):
            pass
        duck = type("Duck", (), {"require_current": lambda self: None})()
        for sdk in ({"toolchain_root": str(self.toolchain)}, duck,
                    object.__new__(ColdSourceIntentSdk), object.__new__(Subclass)):
            with self.subTest(sdk=type(sdk)), self.assertRaises((ERROR, ColdSourceIntent)):
                NAMESPACE["prepare_baseline_lock"](self.authenticated, self.registries,
                    self.output, self.native, sdk)
        self.assertEqual(self.calls, [])

    def test_observed_pair_bytes_and_modes_guarded_on_native_failure(self):
        for name in ("cargo", "rustc"):
            tool = self.toolchain / "bin" / name
            raw = tool.read_bytes()
            def native(_):
                tool.write_bytes(b"changed tool image")
                raise OSError("native failure")
            with self.subTest(name=name), self.assertRaisesRegex(ERROR, "compiler_pair_changed"):
                self.prepare(native)
            tool.write_bytes(raw)
        (self.toolchain / "bin" / "rustc").chmod(0o644)
        with self.assertRaisesRegex(ERROR, "compiler_pair_file"):
            self.prepare()


if __name__ == "__main__":
    unittest.main()
