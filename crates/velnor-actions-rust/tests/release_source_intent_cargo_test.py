"""Mocked fixed Cargo invocation tests; no repository execution or network."""
import json
import hashlib
from pathlib import Path
from types import SimpleNamespace
import tempfile
import sys
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[1] / "src"


def namespace():
    result = {"__name__": "source_intent_cargo_tests"}
    for name in ("release_source_intent_guard.py", "release_source_intent_cargo.py"):
        path = ROOT / name
        exec(compile(path.read_bytes(), str(path), "exec"), result)
    return result


class MockLaunch:
    def __init__(self, paths):
        self.paths = paths
        self.observations = []

    def require_policy(self, rust_version, host):
        if rust_version != "1.98.1" or host != "x86_64-unknown-linux-gnu":
            raise ValueError("source_intent_sdk_policy")

    def installed_tool(self, tool):
        self.observations.append(tool)
        path = self.paths[tool]
        return MockInstalledTool(str(path), hashlib.sha256(path.read_bytes()).hexdigest())


class MockInstalledTool:
    def __init__(self, path, digest):
        self.path, self.sha256 = path, digest
        self.checks = 0

    def require_current(self):
        self.checks += 1


class SourceIntentCargoTests(unittest.TestCase):
    def setUp(self):
        self.ns = namespace()
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-intent-cargo-test-",
                                                      dir=Path("/tmp").resolve())
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def launch(self):
        paths = {tool: self.directory / tool for tool in ("cargo", "rustc")}
        for path in paths.values():
            path.write_bytes(b"mock qualified tool bytes")
        self.ns["ColdSourceIntentSdk"] = MockLaunch
        self.ns["ColdSourceIntentInstalledTool"] = MockInstalledTool
        return MockLaunch(paths)

    def test_environment_literal_excludes_all_ambient_credentials(self):
        with patch.dict("os.environ", {"GH_TOKEN": "secret", "CARGO_NET_GIT_FETCH_WITH_CLI": "true",
                                      "LD_PRELOAD": "hostile", "GITHUB_ENV": "hostile"}):
            environment = self.ns["_intent_environment"](self.directory)
        self.assertEqual(set(environment), {"HOME", "CARGO_HOME", "RUSTUP_HOME", "PATH",
                                            "LC_ALL", "TZ"})
        self.assertEqual(list(Path(environment["PATH"]).iterdir()), [])
        self.assertEqual(list(Path(environment["CARGO_HOME"]).iterdir()), [])
        self.assertNotEqual(environment["HOME"], environment["CARGO_HOME"])

    def test_sdk_without_compiled_qualification_never_executes(self):
        with patch.object(self.ns["subprocess"], "Popen") as run:
            with self.assertRaisesRegex(self.ns["SourceIntentError"],
                                        "ColdSourceIntentSdk"):
                self.ns["source_intent_inventory"]({}, "Cargo.toml", object(),
                                                    "x86_64-unknown-linux-gnu",
                                                    self.directory / "output", "[workspace]\n", {})
        run.assert_not_called()

    def test_metadata_reuses_only_cargo_home_with_other_locations_fresh(self):
        package = self.directory / "package"
        package.mkdir()
        original = self.ns["_intent_environment"](package)
        cargo_home = Path(original["CARGO_HOME"])
        (cargo_home / "registry").mkdir()
        metadata = self.directory / "metadata"
        metadata.mkdir()
        environment = self.ns["_intent_environment"](metadata, original.cargo_home)
        self.assertEqual(environment["CARGO_HOME"], str(cargo_home))
        for key in ("HOME", "RUSTUP_HOME", "PATH"):
            self.assertNotEqual(environment[key], original[key])
            self.assertEqual(list(Path(environment[key]).iterdir()), [])
        self.assertFalse((metadata / "cargo-home").exists())

    def test_metadata_cargo_home_rejects_paths_configs_and_credentials(self):
        environment = self.ns["_intent_environment"](self.directory)
        cargo_home = Path(environment["CARGO_HOME"])
        link = self.directory / "link"
        link.symlink_to(cargo_home, target_is_directory=True)
        for path in (Path("cargo-home"), link, self.directory / "absent"):
            with self.subTest(path=path):
                with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_home"):
                    self.ns["_intent_cargo_home"](path)
        for filename in ("config", "CONFIG.TOML", "credentials", "Credentials.toml"):
            path = cargo_home / filename
            path.write_bytes(b"hostile")
            with self.subTest(filename=filename):
                with self.assertRaisesRegex(self.ns["SourceIntentError"],
                                            "cargo_home_configuration"):
                    self.ns["_intent_cargo_home"](environment.cargo_home)
            path.unlink()

    def test_warm_source_cache_cannot_be_adopted_or_environment_mutated(self):
        warm = self.directory / "warm"
        (warm / "registry/src/hostile").mkdir(parents=True)
        (warm / "registry/src/hostile/.cargo-ok").write_bytes(b"{}")
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_home_capability"):
            self.ns["_intent_cargo_home"](warm)
        environment = self.ns["_intent_environment"](self.directory)
        with self.assertRaises(TypeError):
            environment["CARGO_HOME"] = str(warm)
        with self.assertRaises(AttributeError):
            environment.cargo_home = warm

    def test_replaced_or_moved_cargo_home_rejected_before_spawn(self):
        environment = self.ns["_intent_environment"](self.directory)
        cargo_home = Path(environment["CARGO_HOME"])
        cargo_home.rename(self.directory / "old-home")
        with patch.object(self.ns["subprocess"], "Popen") as spawn:
            with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_home"):
                self.ns["_intent_capture"]([], self.directory, environment)
            cargo_home.mkdir()
            with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_home_identity"):
                self.ns["_intent_capture"]([], self.directory, environment)
        spawn.assert_not_called()

    def test_inventory_derives_forge_from_full_preexecution_official_metadata(self):
        launch = self.launch()
        approved = {"tools": {"rust": "1.98.1"}, "packages": {"demo": "1.0.0"},
                    "tags": {"demo": "demo-v1.0.0"}}
        source, full = object(), {}
        self.ns["require"] = self.ns["_intent_require"]
        for name in ("release_prepare_notes.py", "release_source_intent_descriptor.py"):
            path = ROOT.parents[1] / "velnor-actions-orchestrator/src" / name
            exec(compile(path.read_bytes(), str(path), "exec"), self.ns)
        forge = Mock(wraps=self.ns["forge_release_intent"])
        reader = Mock(return_value=b"## 1.0.0\n\nOfficial source notes.\n")
        config = ('[workspace]\nrelease=false\nrelease_always=false\nsemver_check=true\n'
                  'publish_no_verify=false\npublish_allow_dirty=false\n'
                  'git_tag_name="{{ package }}-v{{ version }}"\n')
        config += ''.join(f'[[package]]\nname="{name}"\nrelease=true\npublish=true\n'
                          'git_only=false\n' for name in ("demo", "other"))
        calls = []

        def materialize(cap, policy, root):
            root.mkdir()
            full.update(workspace_root=str(root), workspace_members=["demo-id", "other-id"],
                packages=[{"id": name + "-id", "name": name, "version": "1.0.0",
                           "manifest_path": str(root / name / "Cargo.toml")}
                          for name in ("demo", "other")])
            return root

        def command(sdk, argv, directory, environment):
            calls.append((list(argv), environment.cargo_home))
            if argv[0] == "package":
                archive = directory / "target/package/demo-1.0.0.crate"
                archive.parent.mkdir(parents=True)
                archive.write_bytes(b"unchanged original")
                return b""
            return json.dumps(full).encode()

        canonical = Mock(return_value={"publish_metadata": {"deps": []}})
        with patch.dict(self.ns, {"load_authenticated_source_snapshot": lambda: source,
                "authenticated_source_descriptor": lambda cap: {},
                "same_json": lambda left, right: left == right,
                "authenticated_source_file": reader,
                "source_intent_content_inventory": Mock(), "_safe_relative_manifest": str,
                "_intent_materialize": materialize, "_intent_command": command,
                "forge_release_intent": forge, "_intent_canonical_package": canonical,
                "selected_publication_order": lambda packages: list(packages)}):
            packages = self.ns["source_intent_inventory"](approved, "Cargo.toml", launch,
                "x86_64-unknown-linux-gnu", self.directory / "output", config, {})
        self.assertEqual(forge.call_args.args[:3],
                         (full, source, {"approved": approved, "release_config": config}))
        self.assertIs(type(forge.call_args.args[3]), str)
        self.assertEqual(packages["demo"]["forge_release"], {
            "tag_name": "demo-v1.0.0", "name": "demo-v1.0.0", "draft": False,
            "prerelease": False, "body": "Official source notes."})
        reader.assert_called_once_with(source, "demo/CHANGELOG.md", limit=1024 * 1024)
        self.assertEqual(calls[1][0][:5],
                         ["metadata", "--locked", "--offline", "--no-deps", "--format-version"])
        self.assertIs(calls[0][1], calls[1][1])
        self.assertIs(canonical.call_args.args[-1], calls[0][1])
        self.assertEqual((self.directory / "output/crates/demo-1.0.0.crate").read_bytes(),
                         b"unchanged original")

    def test_exact_source_artifact_transport_mismatch_precedes_materialize_or_cargo(self):
        launch = self.launch()
        original = {"repository": "a/b", "source_sha": "a" * 40, "tree_sha": "b" * 40,
                    "inner_sha256": "c" * 64, "artifact_id": 5, "attempt": 1}
        for key, replacement in (("tree_sha", "d" * 40), ("inner_sha256", "e" * 64),
                                 ("artifact_id", 6), ("attempt", 2)):
            changed = {**original, key: replacement}
            materialize, command = Mock(), Mock()
            with self.subTest(key=key), patch.dict(self.ns, {
                    "load_authenticated_source_snapshot": lambda: object(),
                    "authenticated_source_descriptor": lambda cap: changed,
                    "same_json": lambda left, right: left == right,
                    "_intent_materialize": materialize, "_intent_command": command}):
                with self.assertRaisesRegex(self.ns["SourceIntentError"], "source_artifact_changed"):
                    self.ns["source_intent_inventory"]({"tools": {"rust": "1.98.1"}},
                        "Cargo.toml", launch, "x86_64-unknown-linux-gnu",
                        self.directory / "output", "[workspace]\n", original)
            materialize.assert_not_called()
            command.assert_not_called()

    def test_relative_or_symlink_sdk_never_reaches_qualification(self):
        launch = self.launch()
        target = self.directory / "actual"
        target.write_bytes(b"sdk")
        link = self.directory / "link"
        link.symlink_to(target)
        for value in (Path("cargo"), link):
            launch.paths["cargo"] = value
            with self.subTest(value=value), patch.object(launch, "installed_tool",
                    return_value=MockInstalledTool(str(value), "0" * 64)):
                with self.assertRaisesRegex(self.ns["SourceIntentError"], "sdk_path"):
                    self.ns["_intent_sdk"](launch)

    def test_fixed_command_has_absolute_compiler_and_empty_wrappers(self):
        environment = self.ns["_intent_environment"](self.directory)
        launch = self.launch()
        cargo, rustc = launch.paths["cargo"], launch.paths["rustc"]
        arguments = ["package", "--locked", "--no-verify", "--allow-dirty"]
        run = Mock(return_value=b"{}")
        with patch.dict(self.ns, {"_intent_capture": run}):
            self.ns["_intent_command"](launch, arguments, self.directory, environment)
        argv = run.call_args.args[0]
        self.assertEqual(argv[:5], [str(cargo), *arguments])
        self.assertIn("build.rustc=" + json.dumps(str(rustc)), argv)
        self.assertIn('build.rustc-wrapper=""', argv)
        self.assertIn('build.rustc-workspace-wrapper=""', argv)
        self.assertEqual(run.call_args.args[1:], (self.directory, environment))
        self.assertEqual(launch.observations, ["cargo", "rustc"])

    def test_failed_cargo_never_yields_semantic_metadata(self):
        environment = self.ns["_intent_environment"](self.directory)
        argv = [sys.executable, "-I", "-S", "-c", 'print("forged metadata"); raise SystemExit(1)']
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_failed"):
            self.ns["_intent_capture"](argv, self.directory, environment)

    def test_duck_typed_or_digest_mismatched_launch_rejected(self):
        launch = self.launch()
        duck = SimpleNamespace(installed_tool=launch.installed_tool)
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "sdk_capability"):
            self.ns["_intent_sdk"](duck)
        with patch.object(launch, "installed_tool",
                          return_value=MockInstalledTool(str(launch.paths["cargo"]), "0" * 64)):
            with self.assertRaisesRegex(self.ns["SourceIntentError"], "sdk_digest"):
                self.ns["_intent_sdk"](launch)
        with patch.object(launch, "installed_tool",
                          return_value=(str(launch.paths["cargo"]), "0" * 64)):
            with self.assertRaisesRegex(self.ns["SourceIntentError"], "sdk_tool_capability"):
                self.ns["_intent_sdk"](launch)

    def test_path_cannot_supply_authenticated_source_authority(self):
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "authenticated_source_snapshot"):
            self.ns["_intent_materialize"](self.directory, {}, self.directory / "source")
        self.assertFalse((self.directory / "source").exists())

    def test_actual_source_loader_unqualified_never_invokes_cargo(self):
        bridge = ROOT.parents[1] / "velnor-actions-orchestrator/src/release_source_artifact_input.py"
        self.ns["ReconcileError"] = self.ns["SourceIntentError"]
        exec(compile(bridge.read_bytes(), str(bridge), "exec"), self.ns)
        launch = self.launch()
        with patch.object(self.ns["subprocess"], "Popen") as run:
            with self.assertRaisesRegex(self.ns["SourceIntentError"],
                                        "source_artifact_compiled_input_unqualified"):
                self.ns["source_intent_inventory"]({"tools": {"rust": "1.98.1"}},
                                                    "Cargo.toml", launch,
                                                    "x86_64-unknown-linux-gnu",
                                                    self.directory / "output", "[workspace]\n", {})
        run.assert_not_called()

    def test_policy_mismatch_precedes_source_load_and_any_installed_tool(self):
        launch = self.launch()
        loader = Mock()
        for rust, host in (("1.99.0", "x86_64-unknown-linux-gnu"),
                           ("1.98.1", "aarch64-unknown-linux-gnu"),
                           ("1.98.1", "aarch64-apple-darwin")):
            with self.subTest(rust=rust, host=host), patch.dict(self.ns, {
                    "load_authenticated_source_snapshot": loader}):
                with self.assertRaisesRegex(ValueError, "source_intent_sdk_policy"):
                    self.ns["source_intent_inventory"]({"tools": {"rust": rust}}, "Cargo.toml",
                                                        launch, host, self.directory / "output", "[workspace]\n", {})
        loader.assert_not_called()
        self.assertEqual(launch.observations, [])

    def test_capture_limits_each_stream_before_allocating_unbounded_output(self):
        environment = self.ns["_intent_environment"](self.directory)
        for stream in ("stdout", "stderr"):
            argv = [sys.executable, "-I", "-S", "-c",
                    'import sys; sys.' + stream + '.write("x" * 10000)']
            with self.subTest(stream=stream):
                with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_output"):
                    self.ns["_intent_capture"](argv, self.directory, environment, limit=100)

    def test_capture_deadline_stops_silent_child(self):
        environment = self.ns["_intent_environment"](self.directory)
        argv = [sys.executable, "-I", "-S", "-c", "import time; time.sleep(10)"]
        with self.assertRaisesRegex(self.ns["SourceIntentError"], "cargo_timeout"):
            self.ns["_intent_capture"](argv, self.directory, environment, timeout=0.05)


if __name__ == "__main__":
    unittest.main()
