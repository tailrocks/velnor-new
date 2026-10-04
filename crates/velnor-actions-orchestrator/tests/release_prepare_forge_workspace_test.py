"""Source-tree Cargo workspace discovery regressions; no repository execution."""
import os
import unittest
from unittest.mock import patch
import release_prepare_forge_test as fixture


class PreparationWorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.case = fixture.PreparationForgeTests()
        self.case.setUp()

    def test_automatic_path_workspace_member_is_accepted(self):
        case = self.case
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\n')
        case.assertEqual(case.run_prepare()["status"], "prepared")


    def test_excluded_automatic_workspace_member_is_rejected(self):
        case = self.case
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\nexclude = ["crates/a"]\n')
        case.rejects("prepare_package_scope")


    def test_automatic_member_cannot_cross_nested_workspace(self):
        case = self.case
        case.before["crates/a/Cargo.toml"] += b'[workspace]\n'
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\n')
        case.rejects("prepare_workspace_member_root")


    def test_automatic_member_closure_is_transitive(self):
        case = self.case
        case.before["crates/a/Cargo.toml"] += b'[dependencies]\nc = { path = "../c", version = "1.0.0" }\n'
        case.before["crates/c/Cargo.toml"] = b'[package]\nname = "c"\nversion = "1.0.0"\n'
        case.before["crates/c/src/lib.rs"] = b'pub fn transitive() {}\n'
        case.after["crates/a/Cargo.toml"] = case.before["crates/a/Cargo.toml"].replace(
            b'version = "1.0.0"\n', b'version = "1.1.0"\n', 1)
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\n')
        case.proposal["manifests"]["c"] = "crates/c/Cargo.toml"
        case.assertEqual(case.run_prepare()["status"], "prepared")
        with patch.dict(os.environ, case.env, clear=True):
            case.assertIn("crates/c/Cargo.toml", case.scope["prepare_scope"](case.approved, case.entries)[1])


    def test_automatic_package_workspace_must_point_to_same_root(self):
        case = self.case
        case.before["crates/a/Cargo.toml"] += b'workspace = "../b"\n'
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\n')
        case.rejects("prepare_workspace_member_root")


    def test_requested_nested_member_uses_nested_workspace_root(self):
        case = self.case
        case.before = {"nested/" + path: raw for path, raw in case.before.items()}
        case.after = {"nested/" + path: raw for path, raw in case.after.items()}
        case.blobs = {case.blob_sha(raw): raw for raw in case.before.values()}
        case.entries = {path: {"sha": case.blob_sha(raw), "mode": "100644", "type": "blob"}
                        for path, raw in case.before.items()}
        case.env["RELEASE_MANIFEST"] = "nested/crates/a/Cargo.toml"
        case.proposal["workspace_manifest"] = "nested/Cargo.toml"
        case.proposal["manifests"] = {name: "nested/" + path for name, path in case.proposal["manifests"].items()}
        case.proposal["files"] = case.files(case.after)
        case.assertEqual(case.run_prepare()["status"], "prepared")


    def test_literal_member_prefix_overrides_exclusion(self):
        case = self.case
        case.set_root_source(b'[workspace]\nmembers = ["crates/a", "crates/b"]\nexclude = ["crates/a"]\n')
        case.assertEqual(case.run_prepare()["status"], "prepared")


    def test_exclusion_is_literal_not_glob(self):
        case = self.case
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\nexclude = ["crates/*"]\n')
        case.assertEqual(case.run_prepare()["status"], "prepared")


    def test_unused_workspace_dependency_is_not_member(self):
        case = self.case
        case.before["unused/Cargo.toml"] = b'[package]\nname = "unused"\nversion = "1.0.0"\n'
        case.set_root_source(b'[workspace]\nmembers = ["crates/b"]\n[workspace.dependencies]\nunused = { path = "unused" }\n')
        case.assertEqual(case.run_prepare()["status"], "prepared")


    def test_unmatched_member_directory_fails(self):
        case = self.case
        case.set_root_source(b'[workspace]\nmembers = ["missing/*", "crates/b"]\n')
        case.rejects("prepare_workspace_member_missing")


    def test_nested_inherited_dependency_is_used_and_resolved_from_workspace(self):
        case = self.case
        root = b'[workspace]\nmembers = ["crates/b"]\n[workspace.dependencies]\na = { path = "crates/a", version = "1.0.0" }\n'
        case.before["crates/b/Cargo.toml"] = b'[package]\nname = "b"\nversion = "1.0.0"\n[dependencies]\na.workspace = true\n'
        case.after.pop("crates/b/Cargo.toml")
        case.after["Cargo.toml"] = root.replace(b'1.0.0', b'1.1.0')
        case.set_root_source(root)
        case.before = {"nested/" + path: raw for path, raw in case.before.items()}
        case.after = {"nested/" + path: raw for path, raw in case.after.items()}
        case.blobs = {case.blob_sha(raw): raw for raw in case.before.values()}
        case.entries = {path: {"sha": case.blob_sha(raw), "mode": "100644", "type": "blob"}
                        for path, raw in case.before.items()}
        case.env["RELEASE_MANIFEST"] = "nested/crates/b/Cargo.toml"
        case.proposal["workspace_manifest"] = "nested/Cargo.toml"
        case.proposal["manifests"] = {name: "nested/" + path for name, path in case.proposal["manifests"].items()}
        case.proposal["files"] = case.files(case.after)
        self.assertEqual(case.run_prepare()["status"], "prepared")

    def test_standalone_manifest_has_only_selected_package(self):
        case = self.case
        case.before.pop("Cargo.toml")
        case.before["crates/a/Cargo.lock"] = case.before["Cargo.lock"]
        case.after["crates/a/Cargo.lock"] = case.after.pop("Cargo.lock")
        case.after.pop("crates/b/Cargo.toml")
        case.blobs = {case.blob_sha(raw): raw for raw in case.before.values()}
        case.entries = {path: {"sha": case.blob_sha(raw), "mode": "100644", "type": "blob"}
                        for path, raw in case.before.items()}
        case.env["RELEASE_MANIFEST"] = "crates/a/Cargo.toml"
        case.proposal["workspace_manifest"] = "crates/a/Cargo.toml"
        case.proposal["manifests"] = {"a": "crates/a/Cargo.toml"}
        case.proposal["files"] = case.files(case.after)
        self.assertEqual(case.run_prepare()["status"], "prepared")
        self.assertEqual(case.pr["title"], "chore: release v1.1.0")

    def test_active_cached_root_precedes_unvisited_nested_workspace(self):
        case = self.case
        case.before["nested/Cargo.toml"] = b'[workspace]\nmembers = ["pkg"]\n'
        case.before["nested/pkg/Cargo.toml"] = b'[package]\nname = "a"\nversion = "1.0.0"\n'
        case.before["nested/pkg/src/lib.rs"] = b'pub fn nested() {}\n'
        case.set_root_source(b'[workspace]\nmembers = ["nested/pkg"]\n')
        with patch.dict(os.environ, case.env, clear=True):
            selected, _, _, _, root = case.scope["prepare_scope"](case.approved, case.entries)
        self.assertEqual(root, "Cargo.toml")
        self.assertEqual(selected, {"a": "nested/pkg/Cargo.toml"})


if __name__ == "__main__":
    unittest.main()
