"""Anonymous release package owner tests; all Cargo/Git calls are mocked."""

import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1] / "src"
COMMON = ROOT.parents[1] / "velnor-actions-orchestrator" / "src"
NAMESPACE = {"__name__": "release_package_test"}
for filename in ("release_reconcile_common.py", "release_source_validation.py",
                 "release_reconcile_cargo.py", "release_preflight_cargo.py",
                 "release_package.py"):
    source_root = COMMON if filename == "release_reconcile_common.py" else ROOT
    exec(compile((source_root / filename).read_text(), filename, "exec"), NAMESPACE)

SHA = "a" * 40
POLICY = {
    "schema": 1,
    "repository": "owner/repo",
    "registry": "crates-io",
    "source_sha": SHA,
    "packages": {"demo": "1.0.0"},
    "owners": {"demo": ["user:1"]},
    "tags": {"demo": "demo-v1.0.0"},
    "authentication": "trusted-publishing",
    "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")},
    "intent_id": "intent-1",
}


class AnonymousPackageTests(unittest.TestCase):
    def test_package_evidence_is_source_bound_and_has_no_forge_call(self):
        with patch.dict(os.environ, {
            "RELEASE_RECONCILE_POLICY": json.dumps(POLICY),
            "GITHUB_SHA": "b" * 40,
            "GITHUB_RUN_ID": "123",
            "GITHUB_RUN_ATTEMPT": "2",
            "RELEASE_MANIFEST": "Cargo.toml",
            "RELEASE_RUST_TOOLCHAIN": "1.98.1",
            "GH_TOKEN": "secret",
            "CARGO_REGISTRY_TOKEN": "secret",
            "GITHUB_OUTPUT": "/tmp/hostile-output",
        }, clear=False):
            self.assertNotIn("forge_package", NAMESPACE["create_package_evidence"].__code__.co_names)

    def test_package_main_writes_candidate_without_registry_or_forge(self):
        values = {
            "RELEASE_RECONCILE_POLICY": json.dumps(POLICY),
            "GITHUB_SHA": "b" * 40,
            "GITHUB_RUN_ID": "123",
            "GITHUB_RUN_ATTEMPT": "2",
            "RELEASE_MANIFEST": "Cargo.toml",
            "RELEASE_RUST_TOOLCHAIN": "1.98.1",
            "GH_TOKEN": "secret",
            "CARGO_REGISTRY_TOKEN": "secret",
        }
        with self._temporary_working_directory() as directory, patch.dict(os.environ, values, clear=False), \
                patch.dict(NAMESPACE, {
                    "validate_source": lambda: None,
                    "verify_source_checkout": lambda *_: None,
                    "approved_package_inventory": lambda *_: {"demo": {"files": {}}},
                }):
            (directory / "release-source").mkdir()
            NAMESPACE["create_package_evidence"]()
            evidence = json.loads((directory / "release-package/evidence.json").read_text())
        self.assertEqual(evidence["status"], "package-verified")
        self.assertEqual(evidence["policy"], POLICY)
        self.assertEqual(evidence["workflow_sha"], "b" * 40)
        self.assertEqual(set(evidence), {
            "schema", "policy", "packages", "workflow_sha", "run_id", "run_attempt", "status"
        })

    def test_git_identity_calls_scrub_token_and_git_selectors(self):
        calls = []

        def run(argv, **kwargs):
            calls.append((argv, kwargs))
            output = SHA if argv[-2:] == ["rev-parse", "HEAD"] else ""
            return SimpleNamespace(stdout=output)

        with patch.dict(os.environ, {
            "RELEASE_RUST_TOOLCHAIN": "1.98.1",
            "GH_TOKEN": "secret",
            "GITHUB_TOKEN": "secret",
            "CARGO_REGISTRY_TOKEN": "secret",
            "GIT_DIR": "/tmp/hostile",
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "core.fsmonitor",
            "GIT_CONFIG_VALUE_0": "/tmp/hostile-hook",
            "GITHUB_OUTPUT": "/tmp/output",
        }, clear=False), patch.object(NAMESPACE["subprocess"], "run", side_effect=run):
            NAMESPACE["verify_source_checkout"](Path("/approved-source"), SHA)
        self.assertEqual(len(calls), 2)
        for _, kwargs in calls:
            self.assertNotIn("GH_TOKEN", kwargs["env"])
            self.assertNotIn("GITHUB_TOKEN", kwargs["env"])
            self.assertNotIn("CARGO_REGISTRY_TOKEN", kwargs["env"])
            self.assertNotIn("GIT_DIR", kwargs["env"])
            self.assertNotIn("GIT_CONFIG_COUNT", kwargs["env"])
            self.assertNotIn("GITHUB_OUTPUT", kwargs["env"])

    def test_cargo_package_keeps_normal_verification_in_anonymous_process(self):
        calls = []
        def run(argv, **kwargs):
            calls.append((argv, kwargs))
            target = Path(argv[argv.index("--target-dir") + 1]) / "package"
            target.mkdir(parents=True)
            (target / "demo-1.0.0.crate").write_bytes(b"verified archive")
            return SimpleNamespace(returncode=0, stderr=b"")
        with patch.dict(os.environ, {"RELEASE_RUST_TOOLCHAIN": "1.98.1", "GH_TOKEN": "secret"}), \
                patch.object(NAMESPACE["subprocess"], "run", side_effect=run), \
                patch.dict(NAMESPACE, {"inventory": lambda *_: {"verified": True}}):
            result = NAMESPACE["approved_package_inventory"](POLICY, Path("/approved"), Path("Cargo.toml"))
        self.assertEqual(result, {"demo": {"verified": True}})
        self.assertIn("--locked", calls[0][0])
        self.assertNotIn("--no-verify", calls[0][0])
        self.assertNotIn("GH_TOKEN", calls[0][1]["env"])

    def test_cargo_verification_failure_blocks_evidence(self):
        with patch.dict(os.environ, {"RELEASE_RUST_TOOLCHAIN": "1.98.1"}), \
                patch.object(NAMESPACE["subprocess"], "run",
                             return_value=SimpleNamespace(returncode=101, stderr=b"dependency compile failed")):
            with self.assertRaisesRegex(NAMESPACE["ReconcileError"], "approved_cargo_package_failed:dependency compile failed"):
                NAMESPACE["approved_package_inventory"](POLICY, Path("/approved"), Path("Cargo.toml"))

    @staticmethod
    def _temporary_working_directory():
        import tempfile
        from contextlib import contextmanager

        @contextmanager
        def directory_context():
            with tempfile.TemporaryDirectory() as directory:
                previous = Path.cwd()
                os.chdir(directory)
                try:
                    yield Path(directory)
                finally:
                    os.chdir(previous)

        return directory_context()


if __name__ == "__main__":
    unittest.main()
