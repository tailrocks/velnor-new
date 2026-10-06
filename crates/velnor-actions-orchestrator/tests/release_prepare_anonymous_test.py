"""Anonymous preparation tests; process execution is fully mocked."""
import base64
import hashlib
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1] / "src"
RUST = ROOT.parents[1] / "velnor-actions-rust" / "src"
NS = {"__name__": "preparation_test"}
for directory, filename in [(ROOT, "release_reconcile_common.py"),
                            (RUST, "release_source_validation.py"),
                            (ROOT, "release_prepare_bytes.py"),
                            (ROOT, "release_prepare_notes.py"),
                            (ROOT, "release_prepare_summary.py"),
                            (ROOT, "release_prepare_anonymous.py")]:
    exec(compile((directory / filename).read_text(), filename, "exec"), NS)


class PreparationTests(unittest.TestCase):
    def test_version_and_local_dependency_updates_preserve_manifest_shape(self):
        before = b'[package]\nname="demo"\nversion="1.0.0"\n[dependencies]\nlocal={path="../local",version="1"}\n'
        after = before.replace(b'1.0.0', b'1.1.0').replace(b'version="1"', b'version="1.1"')
        NS["preparation_bytes"](before, after, "crates/demo/Cargo.toml")
        for mutation in [after.replace(b'../local', b'../../evil'),
                         after + b'build="evil.rs"\n']:
            with self.assertRaises(NS["ReconcileError"]):
                NS["preparation_bytes"](before, mutation, "crates/demo/Cargo.toml")

    def test_registry_lock_records_cannot_change(self):
        before = b'version=4\n[[package]]\nname="demo"\nversion="1.0.0"\n[[package]]\nname="external"\nversion="1.0.0"\nsource="registry+https://example.invalid"\nchecksum="abc"\n'
        NS["preparation_bytes"](before, before.replace(b'version="1.0.0"', b'version="1.1.0"', 1), "Cargo.lock")
        with self.assertRaisesRegex(NS["ReconcileError"], "nonversion"):
            NS["preparation_bytes"](before, before.replace(b'version="1.0.0"', b'version="1.1.0"'), "Cargo.lock")

    def test_proposal_rejects_source_edits_deletions_and_new_manifest(self):
        before = {"Cargo.toml": b'[package]\nname="demo"\nversion="1.0.0"\n',
                  "src/lib.rs": b"old"}
        for after in [{**before, "src/lib.rs": b"evil"},
                      {"Cargo.toml": before["Cargo.toml"]},
                      {**before, "other/Cargo.toml": b""}]:
            with self.assertRaises(NS["ReconcileError"]):
                NS["_preparation_files"](before, after, {"Cargo.toml", "other/Cargo.toml"})

    def test_changed_bytes_bind_git_blob_and_raw_digest(self):
        before = {"Cargo.toml": b'[package]\nname="demo"\nversion="1.0.0"\n'}
        after = {"Cargo.toml": before["Cargo.toml"].replace(b"1.0.0", b"1.1.0")}
        files = NS["_preparation_files"](before, after, {"Cargo.toml"})
        value = files["Cargo.toml"]
        self.assertEqual(base64.b64decode(value["after"]), after["Cargo.toml"])
        self.assertEqual(value["sha256"], hashlib.sha256(after["Cargo.toml"]).hexdigest())
        self.assertEqual(value["before"], NS["_git_blob"](before["Cargo.toml"]))

    def test_clean_environment_strips_credentials_and_command_files(self):
        environment = {"RELEASE_RUST_TOOLCHAIN": "1.98.0", "GIT_TOKEN": "secret",
                       "GITHUB_TOKEN": "secret", "ACTIONS_RUNTIME_TOKEN": "secret",
                       "GH_TOKEN": "secret", "GITHUB_OUTPUT": "/tmp/output"}
        with patch.dict(os.environ, environment, clear=True):
            cleaned = NS["_clean_environment"]()
        self.assertFalse(any(key.endswith("_TOKEN") for key in cleaned))
        self.assertNotIn("GITHUB_OUTPUT", cleaned)

    def test_selected_scope_rejects_registry_and_unselected_version_changes(self):
        before = b'[package]\nname="other"\nversion="1.0.0"\n[dependencies]\nexternal="1"\nlocal={path="../demo",version="1"}\n'
        after = before.replace(b'version="1"', b'version="2"')
        NS["preparation_bytes"](before, after, "Cargo.toml", {"local"})
        for mutation in [after.replace(b'version="1.0.0"', b'version="2.0.0"'),
                         after.replace(b'external="1"', b'external="2"'),
                         after.replace(b'version="2"', b'version={malicious="bad"}'),
                         after.replace(b'version="2"', b'version="garbage"')]:
            with self.assertRaises(NS["ReconcileError"]):
                NS["preparation_bytes"](before, mutation, "Cargo.toml", {"local"})

    def test_process_is_argv_only_anonymous(self):
        calls = []
        def run(argv, **kwargs):
            calls.append((argv, kwargs))
            return SimpleNamespace(returncode=0, stdout=b"summary")
        with patch.object(NS["subprocess"], "run", side_effect=run):
            NS["_preparation_run"](["release-plz", "update", "--forge", "github"], Path("."), {})
        self.assertFalse(calls[0][1]["shell"])
        self.assertEqual(calls[0][1]["env"], {})

    def test_snapshot_preserves_unrelated_symlink_target_and_file_modes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source").write_text("source")
            (root / "link").symlink_to("source")
            (root / "directory-link").symlink_to("outside", target_is_directory=True)
            files, modes = NS["_preparation_snapshot"](root)
            self.assertEqual(files["link"], b"source")
            self.assertEqual(files["directory-link"], b"outside")
            self.assertNotEqual(modes["source"], modes["link"])

    def test_scope_binds_nested_cargo_workspace_manifest_and_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            workspace = root / "nested"
            workspace.mkdir()
            manifest = workspace / "crates/demo/Cargo.toml"
            metadata = {"workspace_root": str(workspace)}
            packages = {"demo": {"manifest_path": str(manifest), "version": "1.0.0"}}
            with patch.dict(os.environ, {"RELEASE_MANIFEST": "nested/Cargo.toml"}), \
                    patch.dict(NS, {"_run_metadata": lambda *_: metadata,
                                    "_workspace_packages": lambda *_: packages}):
                allowed, notes, workspace_manifest, manifests, lock = NS["_preparation_scope"](
                    root, {"packages": {"demo": "1.0.0"}})
            self.assertEqual(workspace_manifest, "nested/Cargo.toml")
            self.assertEqual(lock, "nested/Cargo.lock")
            self.assertEqual(manifests, {"demo": "nested/crates/demo/Cargo.toml"})
            self.assertEqual(notes, {"demo": "nested/crates/demo/CHANGELOG.md"})
            self.assertEqual(allowed, {workspace_manifest, lock, manifests["demo"], notes["demo"]})

    def test_update_uses_fixed_config_and_emits_next_version_byte_proposal(self):
        approved = {"repository": "owner/repo", "source_sha": "a" * 40,
                    "packages": {"demo": "1.0.0"}}
        config = ('[workspace]\nrelease=false\nrelease_always=false\nsemver_check=true\n'
                  'publish_no_verify=false\npublish_allow_dirty=false\n'
                  '[[package]]\nname="demo"\nrelease=true\npublish=true\ngit_only=false\n')
        environment = {"RELEASE_PREPARE_CONFIG": config, "RELEASE_RUST_TOOLCHAIN": "1.98.0",
                       "RELEASE_MANIFEST": "Cargo.toml", "GITHUB_SHA": "b" * 40,
                       "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
                       "GITHUB_ACTOR_ID": "17", "RELEASE_DEFAULT_BRANCH": "main",
                       "GH_TOKEN": "secret", "GIT_TOKEN": "secret",
                       "GIT_CLIFF__CHANGELOG__BODY": "{{ remote.username }}"}
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            original = Path.cwd()
            os.chdir(directory)
            try:
                root = Path("release-source")
                root.mkdir()
                manifest = root / "Cargo.toml"
                manifest.write_bytes(b'[package]\nname="demo"\nversion="1.0.0"\n')
                (root / "cliff.toml").write_text('[changelog]\nbody="{{ remote.username }}"\n')
                (root / "Cargo.lock").write_text('version=4\n[[package]]\nname="demo"\nversion="1.0.0"\n')
                def update(argv, cwd, clean):
                    calls.append((argv, clean))
                    self.assertEqual(Path(argv[3]).read_text(), config)
                    self.assertNotIn("GH_TOKEN", clean)
                    self.assertNotIn("GIT_TOKEN", clean)
                    self.assertNotIn("GIT_CLIFF__CHANGELOG__BODY", clean)
                    self.assertIn("--changelog-config", argv)
                    cliff = Path(argv[argv.index("--changelog-config") + 1])
                    self.assertEqual(cliff.read_text(), NS["PREPARATION_CHANGELOG_CONFIG"])
                    self.assertNotIn("remote.username", cliff.read_text())
                    self.assertNotIn("remote.pr_number", cliff.read_text())
                    self.assertFalse(cliff.is_relative_to(root.resolve()))
                    self.assertFalse(Path(clean["CARGO_TARGET_DIR"]).is_relative_to(root.resolve()))
                    manifest.write_bytes(manifest.read_bytes().replace(b"1.0.0", b"1.1.0"))
                    (root / "Cargo.lock").write_text('version=4\n[[package]]\nname="demo"\nversion="1.1.0"\n')
                    (root / "CHANGELOG.md").write_text("## 1.1.0\nNew feature\n")
                    return "\n* `demo`: 1.0.0 -> 1.1.0 (✓ API compatible changes)\n".encode("utf-8")
                with patch.dict(os.environ, environment, clear=True), patch.dict(NS, {
                    "policy": lambda: approved, "validate_source": lambda: None,
                    "_preparation_identity": lambda *_: "c" * 40,
                    "_preparation_scope": lambda *_: ({"Cargo.toml", "Cargo.lock", "CHANGELOG.md"},
                        {"demo": "CHANGELOG.md"}, "Cargo.toml", {"demo": "Cargo.toml"}, "Cargo.lock"),
                    "_preparation_run": update, "_run_metadata": lambda *_: {},
                    "_workspace_packages": lambda *_: {"demo": {"version": "1.1.0"}},
                }):
                    NS["create_preparation_proposal"]()
                proposal = json.loads(Path("release-proposal/evidence.json").read_text())
                self.assertEqual(proposal["packages"]["demo"]["version"], "1.1.0")
                self.assertEqual(proposal["packages"]["demo"]["semver_check"], "compatible")
                self.assertEqual(proposal["packages"]["demo"]["notes"], "New feature")
                self.assertEqual(proposal["actor"], "17")
                self.assertEqual(proposal["source_tree"], "c" * 40)
                self.assertEqual(set(proposal["files"]), {"Cargo.toml", "Cargo.lock", "CHANGELOG.md"})
                self.assertNotIn("--git-token", calls[0][0])
                self.assertEqual(calls[0][0][-4:], ["--repo-url", "https://github.com/owner/repo", "--forge", "github"])
            finally:
                os.chdir(original)


if __name__ == "__main__":
    unittest.main()
