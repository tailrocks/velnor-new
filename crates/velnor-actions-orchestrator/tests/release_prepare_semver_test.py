"""Pure mocked supplied-checker orchestration and binding regressions."""
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


DIRECTORY = Path(__file__).parent.parent / "src"
NAMESPACE = {}
for filename in ("release_reconcile_common.py", "release_prepare_bytes.py",
                 "release_prepare_registry.py", "release_prepare_docs.py",
                 "release_prepare_semver.py"):
    exec(compile((DIRECTORY / filename).read_text(), filename, "exec"), NAMESPACE)
ERROR = NAMESPACE["ReconcileError"]


def report(success=True):
    return {"schema_version": 1, "success": success,
        "native_report_stdout": "  actual native human diagnostic\nfull finding  \n",
        "crates": [{
        "name": "demo", "success": success, "detected_bump": "Minor",
        "required_bump": None if success else "Major", "selected_checks": 1, "skipped_checks": 0,
        "lints": [{"id": "native", "effective_required_update": "Major",
                   "effective_lint_level": "Deny", "findings": 0}]}]}


class SemverBindingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name).resolve()
        self.actual = self.directory / "actual"
        self.registry = self.directory / "registry"
        for root in (self.actual, self.registry):
            root.mkdir()
            (root / "Cargo.toml").write_bytes(b"manifest")
            (root / "Cargo.toml").chmod(0o644)
        self.index = self.directory / "index-version.json"
        self.index.write_bytes(b'{"name":"demo","vers":"1.2.3"}')
        self.authenticated = {
            "package_root": str(self.registry), "index_version_path": str(self.index),
            "index_version_sha256": hashlib.sha256(self.index.read_bytes()).hexdigest(),
            "inventory_sha256": self.inventory_hash()}

    def inventory_hash(self):
        value = NAMESPACE["_semver_registry_inventory"](self.registry)
        return hashlib.sha256(json.dumps(value, sort_keys=True,
                              separators=(",", ":")).encode()).hexdigest()

    def bind(self):
        NAMESPACE["_semver_bind_registry"](self.actual, self.authenticated)

    def test_independent_exact_source_binding(self):
        self.bind()

    def test_extra_baseline_git_file_or_directory_rejected(self):
        git = self.actual / ".git"
        git.write_bytes(b"gitdir: elsewhere")
        with self.assertRaisesRegex(ERROR, "registry_source"):
            self.bind()
        git.unlink()
        git.mkdir()
        with self.assertRaisesRegex(ERROR, "registry_layout"):
            self.bind()

    def test_actual_file_mutation_rejected(self):
        (self.actual / "Cargo.toml").write_bytes(b"different")
        with self.assertRaisesRegex(ERROR, "registry_source"):
            self.bind()

    def test_extra_actual_file_rejected(self):
        (self.actual / "Cargo.lock").write_bytes(b"new lock")
        with self.assertRaisesRegex(ERROR, "registry_source"):
            self.bind()

    def test_extra_empty_directory_rejected(self):
        (self.actual / "build-probe").mkdir()
        with self.assertRaisesRegex(ERROR, "registry_layout"):
            self.bind()

    def test_exact_empty_directory_layout_preserved(self):
        for root in (self.actual, self.registry):
            (root / "native-empty").mkdir()
        self.bind()

    def test_walk_errors_fail_closed(self):
        def walk(*args, **kwargs):
            kwargs["onerror"](PermissionError("unreadable native directory"))
        with patch.object(NAMESPACE["os"], "walk", walk):
            with self.assertRaisesRegex(ERROR, "baseline_walk"):
                self.bind()

    def test_executable_mode_mutation_rejected(self):
        (self.actual / "Cargo.toml").chmod(0o755)
        with self.assertRaisesRegex(ERROR, "registry_source"):
            self.bind()

    def test_baseline_symlink_rejected(self):
        (self.actual / "link").symlink_to(self.actual / "Cargo.toml")
        with self.assertRaisesRegex(ERROR, "baseline_type"):
            self.bind()

    def test_index_mutation_rejected(self):
        self.index.write_bytes(b"changed")
        with self.assertRaisesRegex(ERROR, "index_changed"):
            self.bind()

    def test_registry_mutation_rejected_even_when_actual_matches(self):
        for root in (self.actual, self.registry):
            (root / "Cargo.toml").write_bytes(b"both changed")
        with self.assertRaisesRegex(ERROR, "inventory_changed"):
            self.bind()

    def test_acquired_directory_layout_is_immutable(self):
        acquired = NAMESPACE["_semver_bind_registry"](self.actual, self.authenticated)
        for root in (self.actual, self.registry):
            (root / "same-unauthenticated-directory").mkdir()
        with self.assertRaisesRegex(ERROR, "registry_layout_changed"):
            NAMESPACE["_semver_bind_registry"](self.actual, self.authenticated, acquired)

    def test_checker_consumes_request_file_and_exact_cwd(self):
        def execute(argv, cwd, environment):
            self.assertEqual(argv[:2], ["/qualified/checker", "compare"])
            self.assertEqual(cwd, self.actual)
            self.assertEqual(environment, {})
            self.assertEqual(json.loads(Path(argv[2]).read_bytes()), {"schema_version": 1})
            return 0, json.dumps(report()).encode()
        code, raw, value = NAMESPACE["_semver_checker"](
            "/qualified/checker", "compare", {"schema_version": 1},
            self.directory, self.actual, execute)
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(raw), value)

    def test_unapproved_result_shape_and_errors_rejected(self):
        for result in (b"stdout", (True, b"{}"), (101, b"{}"), (0, b"{}")):
            with self.subTest(result=result):
                directory = Path(tempfile.mkdtemp(dir=self.directory))
                with self.assertRaises(ERROR):
                    NAMESPACE["_semver_checker"]("/fixed", "plan", {}, directory,
                                                 self.actual, lambda *_: result)

    def test_native_status_and_package_bound(self):
        NAMESPACE["_semver_compare_result"](0, report(), "demo")
        NAMESPACE["_semver_compare_result"](100, report(False), "demo")
        for code, value, name in ((100, report(), "demo"), (0, report(), "other"),
                                  (0, dict(report(), success=1), "demo")):
            with self.assertRaises(ERROR):
                NAMESPACE["_semver_compare_result"](code, value, name)

    def test_arbitrary_prior_outcome_fields_rejected(self):
        value = dict(report(), authenticated=True)
        with self.assertRaisesRegex(ERROR, "status"):
            NAMESPACE["_semver_compare_result"](0, value, "demo")

    def test_duplicate_checker_keys_rejected(self):
        with self.assertRaisesRegex(ERROR, "duplicate_json_key"):
            NAMESPACE["_semver_checker"]("/fixed", "plan", {}, self.directory,
                self.actual, lambda *_: (0, b'{"schema_version":1,"schema_version":1}'))

    def test_checker_cannot_rewrite_request(self):
        def execute(argv, *_):
            Path(argv[2]).write_bytes(b"{}")
            return 0, json.dumps(report()).encode()
        with self.assertRaisesRegex(ERROR, "request_changed"):
            NAMESPACE["_semver_checker"]("/fixed", "compare", {"schema_version": 1},
                                         self.directory, self.actual, execute)

    def test_incompatible_requires_actual_native_human_report(self):
        for text in ("", "  \n", None, 123):
            value = dict(report(False), native_report_stdout=text)
            with self.assertRaisesRegex(ERROR, "native_report"):
                NAMESPACE["_semver_compare_result"](100, value, "demo")

    def test_owned_native_report_rejects_malformed_typed_fields(self):
        cases = [("selected_checks", True), ("skipped_checks", False),
                 ("selected_checks", 2**64), ("skipped_checks", -1),
                 ("selected_checks", 0), ("detected_bump", "minor"),
                 ("detected_bump", "Major"), ("required_bump", "Major"),
                 ("required_bump", []), ("required_bump", "garbage")]
        for key, replacement in cases:
            with self.subTest(key=key, replacement=replacement):
                value = report()
                value["crates"][0][key] = replacement
                with self.assertRaises(ERROR):
                    NAMESPACE["_semver_compare_result"](0, value, "demo")

    def test_owned_native_lints_reject_malformed_enums_ids_and_counts(self):
        cases = [("id", ""), ("effective_required_update", "major"),
                 ("effective_required_update", "Patch"), ("effective_lint_level", "deny"),
                 ("effective_lint_level", []), ("findings", True),
                 ("findings", -1), ("findings", 2**64)]
        for key, replacement in cases:
            with self.subTest(key=key, replacement=replacement):
                value = report()
                value["crates"][0]["lints"][0][key] = replacement
                with self.assertRaises(ERROR):
                    NAMESPACE["_semver_compare_result"](0, value, "demo")
        value = report()
        value["crates"][0]["lints"] *= 2
        value["crates"][0]["selected_checks"] = 2
        with self.assertRaisesRegex(ERROR, "checker_lint"):
            NAMESPACE["_semver_compare_result"](0, value, "demo")

    def test_incompatible_cannot_claim_no_required_bump(self):
        value = report(False)
        value["crates"][0]["required_bump"] = None
        with self.assertRaisesRegex(ERROR, "minor_result"):
            NAMESPACE["_semver_compare_result"](100, value, "demo")

    def test_checker_request_mutation_checked_when_executor_raises(self):
        def execute(argv, *_):
            Path(argv[2]).write_bytes(b"mutated")
            raise OSError("executor failed")
        with self.assertRaisesRegex(ERROR, "request_changed"):
            NAMESPACE["_semver_checker"]("/fixed", "compare", {}, self.directory,
                                         self.actual, execute)


class SemverOrchestrationTests(unittest.TestCase):
    inventory_hash = SemverBindingTests.inventory_hash

    def setUp(self):
        SemverBindingTests.setUp(self)
        self.current = self.directory / "current"
        self.current.mkdir()
        (self.current / "Cargo.toml").write_bytes(b"candidate")
        (self.current / "Cargo.lock").write_bytes(b"locked")
        for root in (self.actual, self.registry):
            (root / "Cargo.lock").write_bytes(b"baseline locked")
        self.authenticated["inventory_sha256"] = self.inventory_hash()
        self.lock_contexts = {side: {"format": 1, "workspaceManifest": str(root / "Cargo.toml"),
            "governingLockfile": str(root / "Cargo.lock"), "lockfileBytesBase64":
            base64.b64encode((root / "Cargo.lock").read_bytes()).decode()}
            for side, root in (("current", self.current), ("baseline", self.actual))}
        self.output = self.directory / "output"
        self.output.mkdir()
        self.context = {"cargo_executable": "/sdk/cargo", "rustdoc_toolchain": "rustdoc native",
            "target": "x86_64-unknown-linux-gnu", "features": [],
            "use_default_features": True, "max_jobs": 2, "output_root": str(self.output)}
        self.calls, self.observed, self.doc_contexts = [], {}, {}
        self.result_code = 0

    def metadata(self, root, manifest, name, version, cargo, output, jobs, governing_lock_context, run):
        root = Path(root)
        manifest = Path(manifest)
        if not manifest.is_absolute():
            manifest = root / manifest
        snapshots = {str(path): path.read_bytes() for path in root.iterdir() if path.is_file()}
        value = {"metadata_bytes": json.dumps({"version": version}).encode(),
            "package_id": str(root) + "#" + name + "@" + version,
            "package_metadata": {"name": name, "version": version, "metadata": {}},
            "manifest_path": str(manifest), "workspace_manifest_path": str(manifest),
            "lock_path": str(root / "Cargo.lock"), "input_snapshots": snapshots,
            "source_snapshot": NAMESPACE["_docs_inventory"](root)}
        self.observed[root] = value
        self.calls.append(("metadata", root))
        return value

    def docs(self, root, manifest, name, version, context, governing_lock_context, run):
        self.doc_contexts[root] = context
        self.calls.append(("docs", root))
        path = self.output / (version + ".json")
        raw = json.dumps({"format_version": 60, "crate_version": version}).encode()
        path.write_bytes(raw)
        return dict(self.observed[root], rustdoc_path=str(path), rustdoc_bytes=raw,
                    rustdoc_sha256=hashlib.sha256(raw).hexdigest(), format_version=60)

    def checker(self, argv, cwd, environment):
        mode = argv[1]
        self.calls.append((mode, cwd))
        request = json.loads(Path(argv[2]).read_bytes())
        if mode == "compare":
            self.assertEqual(request["release_type"], "minor")
            for side in ("current", "baseline"):
                self.assertTrue(Path(request[side]["package"]["metadata_path"]).is_file())
                self.assertTrue(Path(request[side]["rustdoc_path"]).is_file())
            return self.result_code, json.dumps(report(self.result_code == 0)).encode()
        value = {"schema_version": 1, "target": request["target"],
            "baseline_index_version_path": request["baseline_index_version_path"],
            "build_environment": {"target_triple": request["target"],
                "cargo_rustflags": "native-rust", "cargo_rustdocflags": "native-doc",
                "toolchain_version": "native"}}
        for side in ("current", "baseline"):
            value[side] = {"package": request[side], "features": [side + "_feature"],
                "use_default_features": side == "current", "target": request["target"],
                "rustdoc_relative_directory": request["target"] + "/doc"}
        return 0, json.dumps(value).encode()

    def prepare(self, checker=None):
        with patch.dict(NAMESPACE, acquire_registry_baseline=lambda *_: self.authenticated,
                        read_locked_package=self.metadata, generate_locked_docs=self.docs):
            return NAMESPACE["prepare_locked_semver"](self.current, "Cargo.toml", "demo",
                "1.3.0", "1.2.3", self.actual, self.output, self.context, "/sdk/checker",
                self.lock_contexts, lambda *_: self.fail("unexpected real tool"),
                checker or self.checker)

    def test_native_plan_drives_generation_before_compare(self):
        value = self.prepare()
        self.assertEqual(value["status"], "compatible")
        self.assertEqual([call[0] for call in self.calls],
                         ["metadata", "metadata", "plan", "docs", "docs", "compare"])
        for side, root in (("current", self.current), ("baseline", self.actual)):
            self.assertEqual(self.doc_contexts[root]["features"], [side + "_feature"])
            self.assertEqual(self.doc_contexts[root]["native_build_environment"],
                             value["plan"]["build_environment"])
            self.assertEqual(self.doc_contexts[root]["generation_cwd"], str(self.current))
        self.assertFalse(self.doc_contexts[self.actual]["use_default_features"])

    def test_incompatible_retains_full_native_stdout(self):
        self.result_code = 100
        value = self.prepare()
        self.assertEqual(value["status"], "incompatible")
        self.assertEqual(value["report_stdout"], report(False)["native_report_stdout"].strip())

    def test_baseline_mismatch_fails_before_metadata(self):
        (self.actual / "Cargo.toml").write_bytes(b"forged baseline")
        with self.assertRaisesRegex(ERROR, "registry_source"):
            self.prepare()
        self.assertEqual(self.calls, [])

    def test_extra_empty_baseline_directory_fails_before_metadata(self):
        (self.actual / "build-script-probe").mkdir()
        with self.assertRaisesRegex(ERROR, "registry_layout"):
            self.prepare()
        self.assertEqual(self.calls, [])

    def test_extra_baseline_git_fails_before_metadata(self):
        git = self.actual / ".git"
        for kind in ("file", "directory"):
            if kind == "file":
                git.write_bytes(b"gitdir: elsewhere")
            else:
                git.mkdir()
            with self.assertRaisesRegex(ERROR, "registry_(source|layout)"):
                self.prepare()
            self.assertEqual(self.calls, [])
            git.unlink() if kind == "file" else git.rmdir()

    def test_plan_cannot_rebind_native_package(self):
        def checker(*args):
            code, raw = self.checker(*args)
            value = json.loads(raw)
            value["current"]["package"]["package_id"] = "forged"
            return code, json.dumps(value).encode()
        with self.assertRaisesRegex(ERROR, "plan_package"):
            self.prepare(checker)
        self.assertFalse(self.doc_contexts)

    def test_source_mutation_during_plan_fails_before_docs(self):
        def checker(*args):
            result = self.checker(*args)
            (self.current / "Cargo.toml").write_bytes(b"mutated")
            return result
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.prepare(checker)
        self.assertFalse(self.doc_contexts)

    def test_compare_cannot_rewrite_supplied_rustdoc(self):
        def checker(argv, cwd, environment):
            result = self.checker(argv, cwd, environment)
            if argv[1] == "compare":
                request = json.loads(Path(argv[2]).read_bytes())
                Path(request["current"]["rustdoc_path"]).write_bytes(b"mutated")
            return result
        with self.assertRaisesRegex(ERROR, "supplied_input_changed"):
            self.prepare(checker)

    def test_docs_failure_checks_both_sources(self):
        original = self.docs
        def docs(root, *args):
            if root == self.actual:
                (self.current / "Cargo.toml").write_bytes(b"mutated current")
                raise OSError("baseline rustdoc failed")
            return original(root, *args)
        self.docs = docs
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.prepare()

    def test_metadata_failure_checks_source(self):
        def metadata(root, *_):
            (root / "Cargo.toml").write_bytes(b"mutated")
            raise OSError("metadata failed")
        self.metadata = metadata
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.prepare()

    def test_unmodified_failure_preserves_causal_exception(self):
        def checker(*_):
            raise OSError("native failure preserved")
        with self.assertRaisesRegex(OSError, "native failure preserved"):
            self.prepare(checker)


if __name__ == "__main__":
    unittest.main()
