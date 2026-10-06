"""Mocked locked-doc producer proofs; never execute Cargo, Git or network."""
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import unittest


def require(condition, code):
    if not condition:
        raise ValueError(code)


NAMESPACE = {"require": require, "__name__": "release_prepare_docs_test_support"}
SOURCE = Path(__file__).resolve().parents[1] / "src/release_prepare_docs.py"
exec(compile(SOURCE.read_text(), str(SOURCE), "exec"), NAMESPACE)
generate_locked_docs = NAMESPACE["generate_locked_docs"]
read_locked_package = NAMESPACE["read_locked_package"]


class LockedDocsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.base = Path(self.directory.name).resolve()
        self.root = self.base / "source"
        self.root.mkdir()
        self.output = self.base / "output"
        self.output.mkdir()
        self.manifest = self.root / "Cargo.toml"
        self.manifest.write_bytes(b'[package]\nname="sdk"\nversion="1.2.3"\n')
        (self.root / "Cargo.lock").write_bytes(b'version=4\n')
        (self.root / "src").mkdir()
        (self.root / "src/lib.rs").write_bytes(b'pub fn sdk() {}\n')
        (self.root / ".git").mkdir()
        (self.root / ".git/HEAD").write_bytes(b'private git state\n')
        self.package = {"id": "path+file:///sdk#1.2.3", "name": "sdk", "version": "1.2.3",
                        "source": None, "manifest_path": str(self.manifest),
                        "targets": [{"name": "sdk", "kind": ["lib"]}]}
        self.metadata = {"packages": [self.package, {"id": "registry+dep#2.0.0"}],
                         "workspace_root": str(self.root),
                         "workspace_members": [self.package["id"]],
                         "resolve": {"nodes": [{"id": self.package["id"],
                                                "features": ["public"]},
                                               {"id": "registry+dep#2.0.0", "features": []}]}}
        self.document = {"format_version": 55, "crate_version": "1.2.3", "root": 0,
                         "index": {"0": {"name": "sdk"}}}
        self.context = {"cargo_executable": "/trusted/sdk/bin/cargo",
                        "rustdoc_toolchain": "rustdoc 1.98.0-nightly (fixed identity)",
                        "target": "x86_64-unknown-linux-gnu", "features": ["public"],
                        "use_default_features": False, "max_jobs": 2,
                        "output_root": str(self.output),
                        "native_build_environment": {
                            "target_triple": "x86_64-unknown-linux-gnu",
                            "cargo_rustflags": "--cfg exact_native --cap-lints=allow",
                            "cargo_rustdocflags": "--cfg exact_native -Z unstable-options "
                            "--document-private-items --document-hidden-items "
                            "--output-format=json --cap-lints=allow",
                            "toolchain_version": "1.98.0-nightly (fixed identity)"},
                        "rustdoc_relative_directory": "x86_64-unknown-linux-gnu/doc",
                        "generation_cwd": str(self.root)}
        self.lock_context = {"format": 1, "workspaceManifest": str(self.manifest),
                             "governingLockfile": str(self.root / "Cargo.lock"),
                             "lockfileBytesBase64": base64.b64encode(b'version=4\n').decode()}
        self.calls = []
        self.mutate = None

    def mock_run(self, argv, cwd, environment):
        self.calls.append((list(argv), cwd, dict(environment)))
        if argv[1] == "metadata":
            return json.dumps(self.metadata).encode()
        target = Path(argv[argv.index("--target-dir") + 1])
        document = target / self.context["rustdoc_relative_directory"] / "sdk.json"
        document.parent.mkdir(parents=True)
        document.write_bytes(json.dumps(self.document).encode())
        if self.mutate:
            self.mutate()
        return b""

    def generate(self, context=None, run=None):
        return generate_locked_docs(self.root, self.manifest, "sdk", "1.2.3",
                                    context or self.context, self.lock_context, run or self.mock_run)

    def test_full_metadata_and_immutable_inputs_retained(self):
        result = self.generate()
        self.assertEqual(json.loads(result["metadata_bytes"]), self.metadata)
        self.assertEqual(result["package_id"], self.package["id"])
        self.assertEqual(result["package_metadata"], self.package)
        self.assertEqual(result["rustdoc_sha256"],
                         hashlib.sha256(result["rustdoc_bytes"]).hexdigest())
        self.assertEqual(result["format_version"], 55)
        self.assertEqual(result["resolved_features"], ("public",))
        self.assertEqual(result["rustdoc_toolchain"], self.context["rustdoc_toolchain"])
        self.assertEqual(result["input_snapshots"][str(self.root / "Cargo.lock")], b'version=4\n')
        with self.assertRaises(TypeError):
            result["source_snapshot"]["Cargo.lock"] = (0, b"changed")
        with self.assertRaises(TypeError):
            result["input_snapshots"][str(self.manifest)] = b"changed"
        self.assertNotIn(".git/HEAD", result["source_snapshot"])

    def test_commands_use_fixed_capability_offline_resolution_and_selection(self):
        self.generate()
        metadata, rustdoc = [call[0] for call in self.calls]
        self.assertEqual(metadata[:2], [self.context["cargo_executable"], "metadata"])
        self.assertNotIn("--no-deps", metadata)
        self.assertEqual(metadata[metadata.index("--format-version") + 1], "1")
        self.assertEqual(rustdoc[rustdoc.index("--package") + 1], "sdk")
        self.assertNotIn("--", rustdoc)
        for argv, cwd, environment in self.calls:
            self.assertIn("--locked", argv)
            self.assertIn("--offline", argv)
            self.assertIn("--no-default-features", argv)
            self.assertEqual(argv[argv.index("--features") + 1], "public")
            self.assertEqual(cwd, self.root)
            self.assertEqual(set(environment), {"CARGO_TARGET_DIR", "CARGO_BUILD_JOBS",
                                                "CARGO_NET_OFFLINE"} | (set() if argv[1] ==
                                                "metadata" else {"RUSTC_BOOTSTRAP", "RUSTFLAGS",
                                                                 "RUSTDOCFLAGS"}))
            self.assertNotIn(self.root, Path(environment["CARGO_TARGET_DIR"]).parents)

    def test_default_and_empty_feature_selection(self):
        context = dict(self.context, features=[], use_default_features=True)
        self.generate(context=context)
        for argv, _, _ in self.calls:
            self.assertNotIn("--features", argv)
            self.assertNotIn("--no-default-features", argv)

    def test_native_feature_names_preserved_without_python_grammar_subset(self):
        context = dict(self.context, features=["public.dot", "日本語"])
        self.generate(context=context)
        for argv, _, _ in self.calls:
            self.assertEqual(argv[argv.index("--features") + 1], "public.dot,日本語")

    def test_default_metadata_discovery_precedes_native_feature_selection(self):
        result = read_locked_package(self.root, self.manifest, "sdk", "1.2.3",
                                     self.context["cargo_executable"], str(self.output),
                                     2, self.lock_context, self.mock_run)
        self.assertEqual(result["package_id"], self.package["id"])
        self.assertEqual(json.loads(result["metadata_bytes"]), self.metadata)
        self.assertEqual(len(self.calls), 1)
        self.assertNotIn("--features", self.calls[0][0])
        self.assertNotIn("--no-default-features", self.calls[0][0])
        self.assertNotIn("--no-deps", self.calls[0][0])

    def test_native_build_environment_used_without_flag_reinterpretation(self):
        recipe = self.context["native_build_environment"]
        recipe["cargo_rustflags"] = '--cfg "native value" --cap-lints=allow'
        self.generate()
        argv, _, environment = self.calls[1]
        self.assertEqual(environment["RUSTC_BOOTSTRAP"], "1")
        self.assertEqual(environment["RUSTFLAGS"], recipe["cargo_rustflags"])
        self.assertEqual(environment["RUSTDOCFLAGS"], recipe["cargo_rustdocflags"])
        self.assertNotIn("-Z", argv)
        self.assertNotIn("--output-format", argv)

    def test_native_optional_target_and_output_directory(self):
        self.context["target"] = None
        self.context["rustdoc_relative_directory"] = "doc"
        result = self.generate()
        self.assertNotIn("--target", self.calls[1][0])
        self.assertIsNone(result["target"])
        self.assertEqual(result["rustdoc_relative_directory"], "doc")

    def test_generation_uses_shared_current_cwd_and_seals_selected_source(self):
        current = self.base / "current"
        current.mkdir()
        self.context["generation_cwd"] = str(current)
        result = self.generate()
        self.assertEqual(result["generation_cwd"], str(current))
        for _, cwd, _ in self.calls:
            self.assertEqual(cwd, current)
        self.assertIn("src/lib.rs", result["source_snapshot"])

    def test_generation_cwd_requires_absolute_canonical_existing_directory(self):
        for cwd in ("current", str(self.base / "missing"), str(self.manifest)):
            with self.subTest(cwd=cwd):
                self.context["generation_cwd"] = cwd
                with self.assertRaises((ValueError, FileNotFoundError)):
                    self.generate()
        self.assertEqual(self.calls, [])

    def test_native_implicit_target_output_directory(self):
        self.context["target"] = None
        result = self.generate()
        self.assertNotIn("--target", self.calls[1][0])
        self.assertTrue(result["rustdoc_path"].endswith("/x86_64-unknown-linux-gnu/doc/sdk.json"))

    def test_native_recipe_toolchain_mismatch_rejected_before_executor(self):
        self.context["native_build_environment"]["toolchain_version"] = "foreign"
        with self.assertRaisesRegex(ValueError, "locked_docs_toolchain_identity"):
            self.generate()
        self.assertEqual(self.calls, [])

    def test_native_output_traversal_rejected_before_executor(self):
        self.context["rustdoc_relative_directory"] = "../source/doc"
        with self.assertRaisesRegex(ValueError, "locked_docs_relative_directory"):
            self.generate()
        self.assertEqual(self.calls, [])

    def test_safe_dangling_source_link_preserved(self):
        (self.root / "dangling").symlink_to("missing.rs")
        result = self.generate()
        self.assertEqual(result["source_snapshot"]["dangling"][1], b"missing.rs")

    def test_missing_lock_rejected_before_executor(self):
        (self.root / "Cargo.lock").unlink()
        with self.assertRaisesRegex(ValueError, "locked_docs_governing_lock"):
            self.generate()
        self.assertEqual(self.calls, [])

    def test_metadata_identity_mismatches(self):
        for key, wrong in (("name", "foreign"), ("version", "9.0.0"),
                           ("manifest_path", str(self.root / "other/Cargo.toml"))):
            original = self.package[key]
            with self.subTest(key=key):
                self.package[key] = wrong
                with self.assertRaisesRegex(ValueError, "locked_docs_package_identity"):
                    self.generate()
                self.package[key] = original

    def test_missing_dependency_resolution_rejected(self):
        del self.metadata["resolve"]
        with self.assertRaisesRegex(ValueError, "locked_docs_metadata_complete"):
            self.generate()

    def test_registry_package_cannot_replace_native_package(self):
        self.package["source"] = "registry+https://example.invalid/index"
        with self.assertRaisesRegex(ValueError, "locked_docs_native_package"):
            self.generate()

    def test_workspace_lock_required_even_if_other_lock_exists(self):
        workspace = self.root / "workspace"
        workspace.mkdir()
        (workspace / "Cargo.toml").write_bytes(b"[workspace]\n")
        self.metadata["workspace_root"] = str(workspace)
        with self.assertRaisesRegex(ValueError, "locked_docs_governing_workspace_metadata"):
            self.generate()

    def test_source_and_lock_byte_mutations_rejected(self):
        for relative in ("src/lib.rs", "Cargo.lock", "Cargo.toml"):
            path = self.root / relative
            original = path.read_bytes()
            with self.subTest(path=relative):
                self.mutate = lambda: path.write_bytes(b"mutated\n")
                with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
                    self.generate()
                path.write_bytes(original)

    def test_mode_mutation_rejected(self):
        path = self.root / "src/lib.rs"
        self.mutate = lambda: path.chmod(0o700)
        with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
            self.generate()

    def test_symlink_target_mutation_rejected(self):
        link = self.root / "link"
        link.symlink_to("src/lib.rs")

        def mutate():
            link.unlink()
            link.symlink_to("Cargo.toml")

        self.mutate = mutate
        with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
            self.generate()

    def test_new_empty_directory_rejected(self):
        self.mutate = lambda: (self.root / "new-empty").mkdir()
        with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
            self.generate()

    def test_git_state_excluded(self):
        self.mutate = lambda: (self.root / ".git/HEAD").write_bytes(b"changed")
        self.generate()

    def test_escaping_source_link_rejected(self):
        (self.root / "escape").symlink_to(self.output, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "locked_docs_source_link_scope"):
            self.generate()
        self.assertEqual(self.calls, [])

    def test_source_mutation_checked_when_executor_fails(self):
        def fail(argv, cwd, environment):
            (self.root / "src/lib.rs").write_bytes(b"changed")
            raise RuntimeError("executor failed")

        with self.assertRaisesRegex(ValueError, "locked_docs_source_changed"):
            self.generate(run=fail)

    def test_rustdoc_identity_mismatches(self):
        self.document["crate_version"] = "9.0.0"
        with self.assertRaisesRegex(ValueError, "locked_docs_crate_version"):
            self.generate()
        self.document["crate_version"] = "1.2.3"
        self.document["index"]["0"]["name"] = "foreign"
        with self.assertRaisesRegex(ValueError, "locked_docs_crate_name"):
            self.generate()

    def test_external_output_and_closed_context_required(self):
        contexts = [dict(self.context, output_root=str(self.root)),
                    dict(self.context, cargo_executable="cargo"),
                    dict(self.context, authority="self-asserted"),
                    dict(self.context, features=["public", "public"])]
        for context in contexts:
            with self.subTest(context=context):
                with self.assertRaises(ValueError):
                    self.generate(context=context)
        self.assertEqual(self.calls, [])

    def test_missing_or_symlinked_output_rejected(self):
        def no_output(argv, cwd, environment):
            if argv[1] == "metadata":
                return self.mock_run(argv, cwd, environment)
            return b""

        with self.assertRaises(FileNotFoundError):
            self.generate(run=no_output)
        elsewhere = self.output / "forged.json"
        elsewhere.write_text(json.dumps(self.document))

        def linked_output(argv, cwd, environment):
            if argv[1] == "metadata":
                return self.mock_run(argv, cwd, environment)
            path = Path(environment["CARGO_TARGET_DIR"]) / self.context["rustdoc_relative_directory"] / "sdk.json"
            path.parent.mkdir(parents=True)
            path.symlink_to(elsewhere)
            return b""

        with self.assertRaisesRegex(ValueError, "locked_docs_output_link"):
            self.generate(run=linked_output)


if __name__ == "__main__":
    unittest.main()
