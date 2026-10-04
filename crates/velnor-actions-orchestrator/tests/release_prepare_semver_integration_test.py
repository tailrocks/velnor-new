"""Real producer closure; only Cargo/checker/HTTPS transports are synthetic."""
import base64
import copy
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / "src"
NAMESPACE = {"__name__": "release_prepare_semver_integration_support"}
for filename in ("release_reconcile_common.py", "release_prepare_bytes.py",
                 "release_prepare_registry.py", "release_prepare_docs.py",
                 "release_prepare_semver.py"):
    exec(compile((SOURCE / filename).read_text(), filename, "exec"), NAMESPACE)
ERROR = NAMESPACE["ReconcileError"]
TARGET = "x86_64-unknown-linux-gnu"
DECLARED_FEATURES = {"default": ["public"], "public": ["dep:helper"],
                     "unstable": [], "日本語": []}
BUILD = {"target_triple": TARGET, "cargo_rustflags": '--cfg "native value"',
         "cargo_rustdocflags": "-Z unstable-options --output-format=json --cap-lints=allow",
         "toolchain_version": "1.98.0-nightly (fixed identity)"}
HUMAN_REPORT = "  native compatibility report\n  lint: removed public API\n  src/lib.rs:7\n"


def encoded(value):
    return json.dumps(value, ensure_ascii=False).encode()


def native_report(success):
    return {"schema_version": 1, "success": success, "native_report_stdout": HUMAN_REPORT,
            "crates": [{"name": "demo", "success": success, "detected_bump": "Minor",
                        "required_bump": None if success else "Major", "selected_checks": 1,
                        "skipped_checks": 2, "lints": [{"id": "function_missing",
                        "effective_required_update": "Major", "effective_lint_level": "Deny",
                        "findings": 0 if success else 1}]}]}


class RealProducerIntegrationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.roots = {side: self.base / side for side in ("current", "baseline")}
        self.versions = {"current": "1.3.0", "baseline": "1.2.3"}
        for side, root in self.roots.items():
            root.mkdir()
            version = self.versions[side]
            (root / "Cargo.toml").write_text(
                f'[package]\nname="demo"\nversion="{version}"\n'
                '[features]\ndefault=["public"]\npublic=["dep:helper"]\n'
                'unstable=[]\n"日本語"=[]\n')
            (root / "Cargo.lock").write_text(
                f'version=4\n[[package]]\nname="demo"\nversion="{version}"\n')
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text("pub fn public_api() {}\n")
            for path in root.rglob("*"):
                if path.is_file():
                    path.chmod(0o644)
        self.output = self.base / "output"
        self.output.mkdir()
        self.context = {"cargo_executable": "/qualified/cargo", "target": TARGET,
                        "rustdoc_toolchain": "rustdoc " + BUILD["toolchain_version"],
                        "max_jobs": 2, "output_root": str(self.output)}
        self.governing_lock_contexts = {
            side: {"format": 1, "workspaceManifest": str(root / "Cargo.toml"),
                   "governingLockfile": str(root / "Cargo.lock"),
                   "lockfileBytesBase64": base64.b64encode(
                       (root / "Cargo.lock").read_bytes()).decode("ascii")}
            for side, root in self.roots.items()}
        self.cargo_calls, self.checker_calls, self.fetch_calls = [], [], []
        self.code, self.hook = 0, lambda *_: None
        self.metadata_change = lambda metadata, *_: metadata
        self.document_change = lambda document, *_: document
        self.checker_change = lambda value, *_: value
        self.archive = self.make_archive()
        self.index = encoded({"name": "demo", "vers": "1.2.3", "yanked": False,
                              "cksum": hashlib.sha256(self.archive).hexdigest(),
                              "features": DECLARED_FEATURES}) + b"\n"
        for target in ("subprocess.run", "subprocess.Popen", "urllib.request.build_opener"):
            guard = patch(target, side_effect=AssertionError("external transport forbidden"))
            guard.start()
            self.addCleanup(guard.stop)

    def make_archive(self):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            root = self.roots["baseline"]
            for path in sorted(root.rglob("*")):
                if path.is_file():
                    raw = path.read_bytes()
                    member = tarfile.TarInfo("demo-1.2.3/" + path.relative_to(root).as_posix())
                    member.size, member.mode = len(raw), 0o644
                    archive.addfile(member, io.BytesIO(raw))
        return stream.getvalue()

    def fetch(self, url, limit):
        self.fetch_calls.append((url, limit))
        if url == "https://index.crates.io/de/mo/demo":
            return self.index
        self.assertEqual(url, "https://static.crates.io/crates/demo/demo-1.2.3.crate")
        return self.archive

    def metadata(self, side, selected):
        root, version = self.roots[side], self.versions[side]
        identity = "path+file://" + str(root) + "#demo@" + version
        package = {"id": identity, "name": "demo", "version": version, "source": None,
                   "manifest_path": str(root / "Cargo.toml"), "edition": "2024",
                   "features": copy.deepcopy(DECLARED_FEATURES), "metadata": {"native": True},
                   "targets": [{"name": "demo", "kind": ["lib"], "crate_types": ["lib"],
                                "src_path": str(root / "src/lib.rs")}],
                   "dependencies": [{"name": "helper", "optional": True,
                                     "uses_default_features": False, "features": []}]}
        helper = {"id": "registry+helper@2.0.0", "name": "helper", "version": "2.0.0",
                  "source": "registry+https://github.com/rust-lang/crates.io-index"}
        features = (["helper", "public", "日本語"] if side == "current" else ["public"]
                    ) if selected else ["default", "helper", "public"]
        return {"packages": [package, helper], "workspace_root": str(root), "version": 1,
                "workspace_members": [identity], "workspace_default_members": [identity],
                "resolve": {"root": identity, "nodes": [{"id": identity,
                            "features": features, "dependencies": [helper["id"]]},
                            {"id": helper["id"], "features": []}]}}

    def cargo(self, argv, cwd, environment):
        self.cargo_calls.append((list(argv), cwd, dict(environment)))
        self.assertEqual(argv[0], "/qualified/cargo")
        self.assertIn("--locked", argv)
        self.assertIn("--offline", argv)
        self.assertEqual(environment["CARGO_NET_OFFLINE"], "true")
        self.assertEqual(environment["CARGO_BUILD_JOBS"], "2")
        self.assertNotIn("--no-deps", argv)
        manifest = Path(argv[argv.index("--manifest-path") + 1])
        side = next(side for side, root in self.roots.items() if manifest.parent == root)
        selected = "--features" in argv
        if argv[1] == "metadata":
            value = self.metadata_change(self.metadata(side, selected), side, selected)
            self.hook("metadata", side)
            return encoded(value)
        self.assertEqual(argv[1], "rustdoc")
        target = Path(argv[argv.index("--target-dir") + 1])
        self.assertEqual(str(target), environment["CARGO_TARGET_DIR"])
        self.assertFalse(any(root == target or root in target.parents for root in self.roots.values()))
        self.assertEqual(environment["RUSTFLAGS"], BUILD["cargo_rustflags"])
        self.assertEqual(environment["RUSTDOCFLAGS"], BUILD["cargo_rustdocflags"])
        self.assertEqual(environment["RUSTC_BOOTSTRAP"], "1")
        self.assertEqual(argv[argv.index("--target") + 1], TARGET)
        self.assertEqual(argv[argv.index("--package") + 1], "demo")
        self.assertEqual(argv[argv.index("--jobs") + 1], "2")
        self.assertIn("--lib", argv)
        self.assertNotIn("--", argv)
        document = {"format_version": 60, "crate_version": self.versions[side],
                    "root": 0, "index": {"0": {"name": "demo"}}}
        path = target / TARGET / "doc/demo.json"
        path.parent.mkdir(parents=True)
        path.write_bytes(encoded(self.document_change(document, side)))
        self.hook("rustdoc", side)
        return b""

    def checker(self, argv, cwd, environment):
        mode = argv[1]
        self.assertEqual(argv[0], "/qualified/checker")
        self.assertEqual(cwd, self.roots["current"])
        self.assertEqual(environment, {})
        request = json.loads(Path(argv[2]).read_bytes())
        self.checker_calls.append((mode, copy.deepcopy(request)))
        if mode == "compare":
            self.assertEqual(request["release_type"], "minor")
            self.assertEqual(request["current_workspace_manifest_path"],
                             str(self.roots["current"] / "Cargo.toml"))
            for side in self.roots:
                metadata = json.loads(Path(request[side]["package"]["metadata_path"]).read_bytes())
                self.assertEqual(metadata, self.metadata(side, True))
                document = json.loads(Path(request[side]["rustdoc_path"]).read_bytes())
                self.assertEqual(document["crate_version"], self.versions[side])
            value = native_report(self.code == 0)
        else:
            self.assertEqual(mode, "plan")
            self.assertEqual(request["feature_group"], "heuristic")
            self.assertEqual(request["extra_current_features"], [])
            self.assertEqual(request["extra_baseline_features"], [])
            self.assertEqual(json.loads(Path(request["baseline_index_version_path"]).read_bytes()),
                             json.loads(self.index))
            value = {"schema_version": 1, "target": request["target"],
                     "baseline_index_version_path": request["baseline_index_version_path"],
                     "build_environment": dict(BUILD)}
            for side in self.roots:
                package = request[side]
                self.assertEqual(package["package_version"], self.versions[side])
                self.assertEqual(package["manifest_path"], str(self.roots[side] / "Cargo.toml"))
                self.assertEqual(json.loads(Path(package["metadata_path"]).read_bytes()),
                                 self.metadata(side, False))
                value[side] = {"package": package, "features": ["public", "日本語"]
                              if side == "current" else ["public"],
                              "use_default_features": side == "current", "target": TARGET,
                              "rustdoc_relative_directory": TARGET + "/doc"}
        self.hook(mode, request)
        return (self.code if mode == "compare" else 0), encoded(self.checker_change(value, mode))

    def prepare(self):
        return NAMESPACE["prepare_locked_semver"](
            self.roots["current"], "Cargo.toml", "demo", "1.3.0", "1.2.3",
            self.roots["baseline"], self.output, self.context, "/qualified/checker",
            self.governing_lock_contexts, self.cargo, self.checker, self.fetch)

    def test_real_closure_compatible_and_full_incompatible_report(self):
        for code, status in ((0, "compatible"), (100, "incompatible")):
            with self.subTest(code=code):
                self.code = code
                result = self.prepare()
                self.assertEqual(result["status"], status)
                self.assertEqual(result["report"], native_report(code == 0))
                self.assertEqual(result["report_stdout"], HUMAN_REPORT.strip())
                self.assertEqual(result["registry"]["archive_sha256"],
                                 hashlib.sha256(self.archive).hexdigest())
                for side in self.roots:
                    docs = result[side]
                    self.assertEqual(docs["package_metadata"]["features"], DECLARED_FEATURES)
                    self.assertEqual(docs["resolved_features"],
                                     tuple(self.metadata(side, True)["resolve"]["nodes"][0]["features"]))
                    self.assertEqual(docs["input_snapshots"][docs["lock_path"]],
                                     (self.roots[side] / "Cargo.lock").read_bytes())
                    self.assertEqual(dict(docs["governing_lock_context"]),
                                     self.governing_lock_contexts[side])
                    self.assertEqual(docs["generation_cwd"], str(self.roots["current"]))
                    self.assertEqual(dict(docs["native_build_environment"]), BUILD)
                    self.assertEqual(docs["rustdoc_sha256"],
                                     hashlib.sha256(docs["rustdoc_bytes"]).hexdigest())
        self.assertEqual([mode for mode, _ in self.checker_calls], ["plan", "compare"] * 2)
        self.assertEqual(len(self.fetch_calls), 4)

    def test_default_discovery_then_exact_native_feature_commands_and_shared_cwd(self):
        self.prepare()
        self.assertEqual([call[0][1] for call in self.cargo_calls],
                         ["metadata", "metadata", "metadata", "rustdoc", "metadata", "rustdoc"])
        for index, (argv, cwd, _) in enumerate(self.cargo_calls):
            if index < 2:
                self.assertNotIn("--features", argv)
                self.assertNotIn("--no-default-features", argv)
                self.assertEqual(cwd, self.roots["current" if index == 0 else "baseline"])
            else:
                self.assertEqual(cwd, self.roots["current"])
                self.assertEqual(argv[argv.index("--features") + 1],
                                 "public,日本語" if index < 4 else "public")
                self.assertEqual("--no-default-features" in argv, index >= 4)

    def test_baseline_bytes_and_checksum_are_bound_before_cargo(self):
        path = self.roots["baseline"] / "src/lib.rs"
        path.write_text("forged baseline\n")
        with self.assertRaisesRegex(ERROR, "semver_baseline_registry_source"):
            self.prepare()
        self.assertEqual(self.cargo_calls, [])
        path.write_text("pub fn public_api() {}\n")
        self.archive += b"forged"
        with self.assertRaisesRegex(ERROR, "registry_archive_checksum"):
            self.prepare()
        self.assertEqual(self.cargo_calls, [])

    def test_metadata_version_and_source_identity_reject_before_native_plan(self):
        for key, value, reason in (("version", "9.0.0", "locked_docs_package_identity"),
                                   ("source", "registry+forged", "locked_docs_native_package")):
            with self.subTest(key=key):
                def wrong(metadata, *_):
                    metadata["packages"][0][key] = value
                    return metadata
                self.metadata_change = wrong
                with self.assertRaisesRegex(ERROR, reason):
                    self.prepare()
        self.assertEqual(self.checker_calls, [])

    def test_missing_current_lock_rejected_before_cargo(self):
        (self.roots["current"] / "Cargo.lock").unlink()
        with self.assertRaisesRegex(ERROR, "locked_docs_governing_lock"):
            self.prepare()
        self.assertEqual(self.cargo_calls, [])

    def test_governing_lock_dto_binds_exact_bytes_and_workspace_paths(self):
        original = copy.deepcopy(self.governing_lock_contexts)
        cases = (("lockfileBytesBase64", base64.b64encode(b"forged lock").decode()),
                 ("workspaceManifest", str(self.roots["baseline"] / "Cargo.toml")),
                 ("governingLockfile", str(self.roots["baseline"] / "Cargo.lock")),
                 ("lockfileBytesBase64", ""), ("lockfileBytesBase64", "eA"),
                 ("lockfileBytesBase64", "eB=="), ("format", True))
        for key, value in cases:
            with self.subTest(key=key, value=value):
                self.governing_lock_contexts = copy.deepcopy(original)
                self.governing_lock_contexts["current"][key] = value
                with self.assertRaises(ERROR):
                    self.prepare()
                self.assertEqual(self.cargo_calls, [])

    def test_governing_lock_context_mapping_is_closed(self):
        original = copy.deepcopy(self.governing_lock_contexts)
        for value in (None, {}, {"current": original["current"]},
                      dict(original, unknown=original["current"])):
            with self.subTest(value=value):
                self.governing_lock_contexts = value
                with self.assertRaises(ERROR):
                    self.prepare()
                self.assertEqual(self.cargo_calls, [])

    def test_authenticated_archive_traversal_rejected_before_cargo(self):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            member = tarfile.TarInfo("demo-1.2.3/../escape")
            member.size, member.mode = 1, 0o644
            archive.addfile(member, io.BytesIO(b"x"))
        self.archive = stream.getvalue()
        index = json.loads(self.index)
        index["cksum"] = hashlib.sha256(self.archive).hexdigest()
        self.index = encoded(index)
        with self.assertRaisesRegex(ERROR, "registry_archive_path"):
            self.prepare()
        self.assertEqual(self.cargo_calls, [])

    def test_generation_cannot_change_declared_feature_graph(self):
        def wrong(metadata, side, selected):
            if selected:
                metadata["packages"][0]["features"]["public"] = []
            return metadata
        self.metadata_change = wrong
        with self.assertRaisesRegex(ERROR, "semver_generated_source_changed"):
            self.prepare()
        self.assertEqual([mode for mode, _ in self.checker_calls], ["plan"])

    def test_lock_mutation_during_real_rustdoc_detected_on_failure(self):
        def mutate(stage, side):
            if stage == "rustdoc":
                (self.roots[side] / "Cargo.lock").write_bytes(b"changed")
                raise OSError("synthetic tool failed")
        self.hook = mutate
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.prepare()

    def test_compare_cannot_change_supplied_doc_or_index(self):
        for field in ("rustdoc_path", "index"):
            with self.subTest(field=field):
                def mutate(stage, request):
                    if stage == "compare":
                        path = (request["current"]["rustdoc_path"] if field == "rustdoc_path"
                                else self.checker_calls[-2][1]["baseline_index_version_path"])
                        Path(path).write_bytes(b"changed")
                self.hook = mutate
                with self.assertRaisesRegex(ERROR, "supplied_input_changed"):
                    self.prepare()

    def test_real_closure_rejects_boolean_native_count(self):
        def wrong(value, mode):
            if mode == "compare":
                value["crates"][0]["selected_checks"] = True
            return value
        self.checker_change = wrong
        with self.assertRaisesRegex(ERROR, "semver_checker_counts"):
            self.prepare()

    def test_plan_cannot_change_bound_package_version(self):
        def wrong(value, mode):
            if mode == "plan":
                value["baseline"]["package"]["package_version"] = "9.0.0"
            return value
        self.checker_change = wrong
        with self.assertRaisesRegex(ERROR, "semver_plan_package"):
            self.prepare()
        self.assertEqual(len(self.cargo_calls), 2)


if __name__ == "__main__":
    unittest.main()
