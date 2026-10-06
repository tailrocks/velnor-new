"""Executable fixtures for the exact-source release package gate."""

from contextlib import contextmanager
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_source_validation.py"
SPEC = importlib.util.spec_from_file_location("release_source_validation_under_test", SOURCE)
VALIDATION = importlib.util.module_from_spec(SPEC)
if SPEC.loader is None:
    raise RuntimeError("missing validation loader")
SPEC.loader.exec_module(VALIDATION)


class SourceValidationTest(unittest.TestCase):
    """Run the helper against metadata returned by a controlled Cargo stub."""

    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        self.source_root = self.root / "release-source"
        self.package_root = self.source_root / "crates" / "demo"
        self.package_root.mkdir(parents=True)
        (self.source_root / "Cargo.toml").write_text("[workspace]\nmembers = [\"crates/demo\"]\n")
        (self.package_root / "Cargo.toml").write_text(
            "[package]\nname = \"demo\"\nversion = \"1.2.3\"\n"
        )
        self.source_root = self.source_root.resolve()
        self.package_root = self.package_root.resolve()
        self.package_manifest = self.package_root / "Cargo.toml"
        self.package_id = f"path+{self.package_root.as_uri()}#1.2.3"
        self.metadata = {
            "version": 1,
            "workspace_root": str(self.source_root),
            "packages": [
                {
                    "id": self.package_id,
                    "name": "demo",
                    "version": "1.2.3",
                    "manifest_path": str(self.package_manifest.resolve()),
                    "publish": None,
                }
            ],
            "workspace_members": [self.package_id],
        }

    def tearDown(self):
        self.tempdir.cleanup()

    @contextmanager
    def configured(self, expected=None, scope="0"):
        values = {
            "RELEASE_EXPECTED_PACKAGES": json.dumps(expected or {"demo": "1.2.3"}),
            "RELEASE_MANIFEST": "Cargo.toml",
            "RELEASE_PUBLISHABLE_WORKSPACE": scope,
            "RELEASE_REGISTRY": "crates-io",
            "RELEASE_REPOSITORY": "acme/widgets",
            "RELEASE_RUST_TOOLCHAIN": "1.98.1",
        }
        old = os.getcwd()
        try:
            os.chdir(self.root)
            with patch.dict(os.environ, values, clear=False):
                yield
        finally:
            os.chdir(old)

    def run_validation(self, **metadata_changes):
        document = dict(self.metadata)
        document.update(metadata_changes)
        result = subprocess.CompletedProcess(
            ["cargo", "metadata"],
            0,
            stdout=json.dumps(document),
            stderr="",
        )
        with patch.object(VALIDATION.subprocess, "run", return_value=result) as run:
            VALIDATION.validate_source()
        return run

    def test_accepts_exact_source_map_and_scrubs_credentials(self):
        with self.configured():
            with patch.dict(
                os.environ,
                {
                    "GH_TOKEN": "secret",
                    "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "secret",
                    "ACTIONS_ID_TOKEN_REQUEST_URL": "https://token.invalid",
                    "ACTIONS_RUNTIME_TOKEN": "secret",
                    "ACTIONS_CACHE_URL": "https://cache.invalid",
                    "CARGO_REGISTRY_GLOBAL_CREDENTIAL_PROVIDERS": "secret",
                    "CARGO_REGISTRIES_INTERNAL_TOKEN": "secret",
                    "GITHUB_ENV": "/tmp/env",
                    "GITHUB_PATH": "/tmp/path",
                    "GITHUB_OUTPUT": "/tmp/output",
                    "GITHUB_STATE": "/tmp/state",
                    "GITHUB_STEP_SUMMARY": "/tmp/summary",
                    "CARGO_HOME": "/tmp/hostile-cargo-home",
                    "CARGO_BUILD_RUSTC_WRAPPER": "wrapper",
                    "RUSTC": "compiler",
                    "RUSTUP_TOOLCHAIN": "stable",
                    "RUSTFLAGS": "--cfg=hostile",
                    "GIT_DIR": str(self.root / "hostile.git"),
                    "GIT_WORK_TREE": str(self.root / "hostile-worktree"),
                    "GIT_CONFIG_GLOBAL": str(self.root / "hostile.gitconfig"),
                    "GIT_CONFIG_SYSTEM": str(self.root / "hostile-system.gitconfig"),
                    "GIT_CONFIG_NOSYSTEM": "0",
                    "GIT_CONFIG_COUNT": "1",
                    "GIT_CONFIG_KEY_0": "url.file:///attacker/.insteadOf",
                    "GIT_CONFIG_VALUE_0": "https://registry.example/",
                    "GIT_EXTERNAL_DIFF": "attacker-diff",
                    "KEEP_FOR_PROBE": "yes",
                },
                clear=False,
            ):
                run = self.run_validation()
        self.assert_safe_probe(run)

    def assert_safe_probe(self, run):
        kwargs = run.call_args.kwargs
        self.assertFalse(kwargs["shell"])
        self.assertNotEqual(kwargs["cwd"], str(self.source_root.resolve()))
        self.assertTrue(Path(kwargs["cwd"]).is_absolute())
        self.assertTrue(Path(kwargs["env"]["CARGO_HOME"]).is_absolute())
        self.assertNotIn(str(self.source_root.resolve()), kwargs["cwd"])
        self.assertNotIn(str(self.source_root.resolve()), kwargs["env"]["CARGO_HOME"])
        self.assertEqual(kwargs["env"]["RUSTUP_TOOLCHAIN"], "1.98.1")
        self.assertEqual(
            run.call_args.args[0],
            [
                "cargo",
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
                "--config",
                'build.rustc="rustc"',
                "--config",
                'build.rustc-wrapper=""',
                "--config",
                'build.rustc-workspace-wrapper=""',
                "--manifest-path",
                str(self.source_root / "Cargo.toml"),
            ],
        )
        self.assertNotIn("GH_TOKEN", kwargs["env"])
        self.assertNotIn("ACTIONS_ID_TOKEN_REQUEST_TOKEN", kwargs["env"])
        self.assertNotIn("ACTIONS_ID_TOKEN_REQUEST_URL", kwargs["env"])
        self.assertNotIn("ACTIONS_RUNTIME_TOKEN", kwargs["env"])
        self.assertNotIn("ACTIONS_CACHE_URL", kwargs["env"])
        self.assertNotIn("CARGO_REGISTRY_GLOBAL_CREDENTIAL_PROVIDERS", kwargs["env"])
        self.assertNotIn("CARGO_REGISTRIES_INTERNAL_TOKEN", kwargs["env"])
        for key in [
            "GITHUB_ENV",
            "GITHUB_PATH",
            "GITHUB_OUTPUT",
            "GITHUB_STATE",
            "GITHUB_STEP_SUMMARY",
        ]:
            self.assertNotIn(key, kwargs["env"])
        self.assertNotIn("CARGO_BUILD_RUSTC_WRAPPER", kwargs["env"])
        self.assertNotIn("RUSTC", kwargs["env"])
        self.assertEqual(kwargs["env"]["RUSTUP_TOOLCHAIN"], "1.98.1")
        self.assertNotIn("RUSTFLAGS", kwargs["env"])
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_0",
            "GIT_EXTERNAL_DIFF",
        ]:
            self.assertNotIn(key, kwargs["env"])
        self.assertEqual(kwargs["env"]["GIT_CONFIG_NOSYSTEM"], "1")
        self.assertEqual(kwargs["env"]["GIT_CONFIG_GLOBAL"], os.devnull)
        self.assertEqual(kwargs["env"]["KEEP_FOR_PROBE"], "yes")

    def test_clean_environment_blocks_actual_git_config_redirectors(self):
        hostile_config = self.root / "hostile.gitconfig"
        hostile_config.write_text(
            '[url "file:///attacker/"]\n\tinsteadOf = https://registry.example/\n'
        )
        hostile_dir = self.root / "hostile.git"
        hostile_dir.mkdir()
        (hostile_dir / "config").write_text(
            '[url "file:///attacker-local/"]\n\tinsteadOf = https://registry.example/\n'
        )
        with self.configured():
            with patch.dict(
                os.environ,
                {
                    "GIT_DIR": str(hostile_dir),
                    "GIT_CONFIG_GLOBAL": str(hostile_config),
                    "GIT_CONFIG_NOSYSTEM": "0",
                    "GIT_CONFIG_COUNT": "1",
                    "GIT_CONFIG_KEY_0": "url.file:///attacker-command/.insteadOf",
                    "GIT_CONFIG_VALUE_0": "https://registry.example/",
                },
                clear=False,
            ):
                environment = VALIDATION._clean_environment()
        result = subprocess.run(
            ["git", "config", "--get-regexp", r"^url\..*\.insteadOf$"],
            cwd=self.root,
            env=environment,
            capture_output=True,
            check=False,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")

    def test_rejects_version_mismatch(self):
        with self.configured(expected={"demo": "1.2.4"}):
            with self.assertRaisesRegex(VALIDATION.ValidationError, "version_mismatch:demo"):
                self.run_validation()

    def test_rejects_duplicate_expected_package_keys(self):
        with self.configured():
            with patch.dict(
                os.environ,
                {"RELEASE_EXPECTED_PACKAGES": '{"demo":"1.2.3","demo":"1.2.4"}'},
            ):
                with self.assertRaisesRegex(
                    VALIDATION.ValidationError, "duplicate_expected_package"
                ):
                    self.run_validation()

    def test_rejects_nonfinite_expected_package_values(self):
        with self.configured():
            with patch.dict(
                os.environ, {"RELEASE_EXPECTED_PACKAGES": '{"demo":NaN}'},
            ):
                with self.assertRaisesRegex(
                    VALIDATION.ValidationError, "nonfinite_expected_packages:NaN"
                ):
                    self.run_validation()

    def test_rejects_unknown_or_missing_selected_package(self):
        with self.configured(expected={"missing": "1.2.3"}):
            with self.assertRaisesRegex(
                VALIDATION.ValidationError, "missing_selected:missing"
            ):
                self.run_validation()

    def test_rejects_package_id_name_mismatch(self):
        package = dict(self.metadata["packages"][0])
        package["id"] = f"path+{self.package_root.as_uri()}#wrong@1.2.3"
        with self.configured():
            with self.assertRaisesRegex(
                VALIDATION.ValidationError, "package_id_identity_mismatch"
            ):
                self.run_validation(packages=[package])

    def test_rejects_private_selected_package(self):
        private = dict(self.metadata["packages"][0])
        private["publish"] = []
        with self.configured():
            with self.assertRaisesRegex(VALIDATION.ValidationError, "publish_policy:demo"):
                self.run_validation(packages=[private])

    def test_rejects_registry_mismatch(self):
        restricted = dict(self.metadata["packages"][0])
        restricted["publish"] = ["internal"]
        with self.configured():
            with self.assertRaisesRegex(VALIDATION.ValidationError, "publish_policy:demo"):
                self.run_validation(packages=[restricted])

    def test_rejects_workspace_root_outside_source(self):
        with self.configured():
            with self.assertRaisesRegex(
                VALIDATION.ValidationError, "workspace_root_outside_source"
            ):
                self.run_validation(workspace_root=str(self.root.resolve()))

    def test_rejects_symlink_source_root(self):
        real_root = self.root / "release-source-real"
        (self.root / "release-source").rename(real_root)
        (self.root / "release-source").symlink_to(real_root, target_is_directory=True)
        with self.configured():
            with self.assertRaisesRegex(
                VALIDATION.ValidationError, "symlink_source_root"
            ):
                self.run_validation()

    def test_allows_workspace_member_sibling_within_source(self):
        workspace_root = self.source_root / "workspace"
        workspace_root.mkdir()
        (workspace_root / "Cargo.toml").write_text(
            "[workspace]\nmembers = [\"../crates/demo\"]\n"
        )
        with self.configured():
            self.run_validation(workspace_root=str(workspace_root.resolve()))

    def test_publishable_workspace_rejects_extra_publishable_member(self):
        extra_root = self.source_root / "crates" / "extra"
        extra_root.mkdir()
        extra_manifest = extra_root / "Cargo.toml"
        extra_manifest.write_text("[package]\nname = \"extra\"\nversion = \"0.1.0\"\n")
        extra_id = f"path+{extra_root.as_uri()}#extra@0.1.0"
        extra = {
            "id": extra_id,
            "name": "extra",
            "version": "0.1.0",
            "manifest_path": str(extra_manifest),
            "publish": None,
        }
        metadata = dict(self.metadata)
        metadata["packages"] = [self.metadata["packages"][0], extra]
        metadata["workspace_members"] = [self.package_id, extra_id]
        with self.configured(scope="1"):
            with self.assertRaisesRegex(
                VALIDATION.ValidationError, "publishable_scope_mismatch:extra"
            ):
                self.run_validation(**metadata)

    def test_explicit_scope_allows_other_publishable_member(self):
        extra_root = self.source_root / "crates" / "extra"
        extra_root.mkdir()
        extra_manifest = extra_root / "Cargo.toml"
        extra_manifest.write_text("[package]\nname = \"extra\"\nversion = \"0.1.0\"\n")
        extra_id = f"path+{extra_root.as_uri()}#extra@0.1.0"
        extra = {
            "id": extra_id,
            "name": "extra",
            "version": "0.1.0",
            "manifest_path": str(extra_manifest.resolve()),
            "publish": None,
        }
        metadata = dict(self.metadata)
        metadata["packages"] = [self.metadata["packages"][0], extra]
        metadata["workspace_members"] = [self.package_id, extra_id]
        with self.configured(scope="0"):
            self.run_validation(**metadata)


if __name__ == "__main__":
    unittest.main()
