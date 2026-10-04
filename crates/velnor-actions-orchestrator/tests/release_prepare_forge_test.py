"""Mocked forge proofs for fixed preparation; no GitHub writes or local Git."""
import base64
import copy
import hashlib
import json
import os
import subprocess
import zipfile
from pathlib import Path
import unittest
from unittest.mock import patch


class PreparationForgeTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / "src"
        self.scope = {"__name__": "release_prepare_forge_test_runtime"}
        for filename in ("release_reconcile_common.py", "release_reconcile_forge.py",
                         "release_prepare_bytes.py", "release_prepare_notes.py", "release_prepare_forge_workspace.py", "release_prepare_forge_metadata.py", "release_prepare_forge.py"):
            exec(compile((root / filename).read_text(), filename, "exec"), self.scope)
        self.source, self.source_tree = "1" * 40, "2" * 40
        self.new_tree, self.head = "3" * 40, "4" * 40
        self.env = {"GITHUB_RUN_ID": "5", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_SHA": "6" * 40,
                    "GITHUB_ACTOR_ID": "99", "RELEASE_DEFAULT_BRANCH": "main", "RELEASE_MANIFEST": "Cargo.toml",
                    "RELEASE_PREPARE_ARTIFACT_ID": "7", "RELEASE_PREPARE_ARTIFACT_DIGEST": "8" * 64}
        self.approved = {"repository": "owner/repo", "source_sha": self.source,
                         "intent_id": "intent", "packages": {"a": "1.0.0"}}
        self.before = {"Cargo.toml": b'[workspace]\nmembers = ["crates/*"]\n',
                       "Cargo.lock": b'version = 4\n[[package]]\nname = "a"\nversion = "1.0.0"\n[[package]]\nname = "b"\nversion = "1.0.0"\n',
                       "crates/a/Cargo.toml": b'[package]\nname = "a"\nversion = "1.0.0"\n',
                       "crates/b/Cargo.toml": b'[package]\nname = "b"\nversion = "1.0.0"\n[dependencies]\na = { path = "../a", version = "1.0.0" }\n',
                       "crates/a/src/lib.rs": b'pub fn source() {}\n',
                       "crates/b/src/lib.rs": b'pub fn dependent() {}\n'}
        self.after = {"Cargo.lock": self.before["Cargo.lock"].replace(
                          b'name = "a"\nversion = "1.0.0"', b'name = "a"\nversion = "1.1.0"'),
                      "crates/a/Cargo.toml": self.before["crates/a/Cargo.toml"].replace(b'1.0.0', b'1.1.0'),
                      "crates/b/Cargo.toml": self.before["crates/b/Cargo.toml"].replace(b'version = "1.0.0" }', b'version = "1.1.0" }'),
                      "crates/a/CHANGELOG.md": b'# Changelog\n\n## [1.1.0] - 2026-10-03\n\nRelease notes.\n'}
        self.blobs = {self.blob_sha(raw): raw for raw in self.before.values()}
        self.entries = {path: {"sha": self.blob_sha(raw), "mode": "100644", "type": "blob"}
                        for path, raw in self.before.items()}
        self.proposal = {"schema": 1, "policy": self.approved, "source_sha": self.source,
                         "workflow_sha": self.env["GITHUB_SHA"], "run_id": "5", "run_attempt": "2",
                         "actor": "99", "base": "main", "source_tree": self.source_tree,
                         "packages": {"a": {"version": "1.1.0", "notes": "Release notes.", "previous_version": "1.0.0",
                                            "semver_check": "compatible", "breaking_changes": ""}},
                         "files": self.files(self.after), "status": "prepared", "workspace_manifest": "Cargo.toml",
                         "manifests": {"a": "crates/a/Cargo.toml", "b": "crates/b/Cargo.toml"}}
        self.artifact = {"id": 7, "digest": "sha256:" + "8" * 64,
                         "name": "velnor-release-proposal-r5-a2"}
        self.writes, self.ref, self.pr, self.receipts = [], None, None, []
        self.truncated, self.branch_owner = False, 41898282
        self.scope["forge_api"] = self.read
        self.scope["prepare_api"] = self.write
        self.scope["artifact_evidence"] = self.artifact_evidence
        self.scope["save_receipt"] = lambda receipt, path: self.receipts.append(copy.deepcopy(receipt))

    def blob_sha(self, raw):
        return self.scope["prepare_blob_sha"](raw)

    def files(self, after):
        return {path: {"before": self.entries.get(path, {}).get("sha"),
                       "after": base64.b64encode(raw).decode(),
                       "sha256": hashlib.sha256(raw).hexdigest()} for path, raw in after.items()}

    def artifact_evidence(self, approved, name, producer):
        self.assertEqual(name, "velnor-release-proposal-r5-a2")
        self.assertEqual(producer, "release-preparation-source")
        return json.dumps(self.proposal).encode(), self.artifact

    def read(self, endpoint):
        prefix = "repos/owner/repo"
        if endpoint == prefix:
            return {"full_name": "owner/repo", "default_branch": "main"}
        if endpoint == prefix + "/git/ref/heads/main":
            return {"ref": "refs/heads/main", "object": {"type": "commit", "sha": self.source}}
        if endpoint == prefix + "/git/ref/heads/velnor-release%2Fintent":
            return self.ref
        if endpoint.startswith(prefix + "/git/blobs/"):
            sha = endpoint.rsplit("/", 1)[1]
            return {"sha": sha, "encoding": "base64", "content": base64.b64encode(self.blobs[sha]).decode()}
        if endpoint.startswith(prefix + "/git/commits/"):
            sha = endpoint.rsplit("/", 1)[1]
            return {"sha": sha, "tree": {"sha": self.source_tree if sha == self.source else self.new_tree},
                    "parents": [] if sha == self.source else [{"sha": self.source}]}
        if endpoint.startswith(prefix + "/git/trees/"):
            sha = endpoint.rsplit("/", 1)[1].split("?")[0]
            entries = self.entries if sha == self.source_tree else self.expected
            return {"sha": sha, "truncated": self.truncated,
                    "tree": [{"path": path, **entry} for path, entry in entries.items()]}
        if endpoint.startswith(prefix + "/pulls?"):
            return [] if self.pr is None else [self.pr]
        raise AssertionError(endpoint)

    def write(self, endpoint, payload=None, method="POST"):
        self.writes.append((endpoint, payload))
        if endpoint.endswith("/git/blobs"):
            raw = base64.b64decode(payload["content"])
            return {"sha": self.blob_sha(raw)}
        if endpoint.endswith("/git/trees"):
            self.expected = copy.deepcopy(self.entries)
            self.expected.update({entry["path"]: {key: entry[key] for key in ("sha", "mode", "type")}
                                  for entry in payload["tree"]})
            return {"sha": self.new_tree}
        if endpoint.endswith("/git/commits"):
            self.assertEqual(payload["parents"], [self.source])
            return {"sha": self.head}
        if endpoint.endswith("/git/refs"):
            self.ref = {"ref": payload["ref"], "object": {"sha": payload["sha"], "type": "commit"}}
            return self.ref
        if endpoint.endswith("/pulls"):
            self.pr = {"number": 10, "html_url": "https://github.com/owner/repo/pull/10", "state": "open",
                       "user": {"id": self.branch_owner, "login": "github-actions[bot]", "type": "Bot"}, "title": payload["title"], "body": payload["body"],
                       "base": {"ref": "main", "repo": {"full_name": "owner/repo"}},
                       "head": {"ref": payload["head"], "sha": self.head, "repo": {"full_name": "owner/repo"}}}
            return self.pr
        raise AssertionError(endpoint)

    def run_prepare(self):
        with patch.dict(os.environ, self.env, clear=True):
            return self.scope["prepare_pull_request"](self.approved)

    def rejects(self, reason):
        with self.assertRaisesRegex(self.scope["ReconcileError"], reason):
            self.run_prepare()
        self.assertEqual(self.writes, [])

    def test_create_then_reuse_verified_pr(self):
        receipt = self.run_prepare()
        self.assertEqual(receipt["head_sha"], self.head)
        self.assertEqual(receipt["pull_request"], 10)
        self.assertEqual(self.pr["title"], "chore(a): release v1.1.0")
        writes = copy.deepcopy(self.writes)
        self.assertEqual(self.run_prepare(), receipt)
        self.assertEqual(self.writes, writes)

    def test_noop_requires_valid_package_version_and_never_writes(self):
        self.proposal["files"] = {}
        self.proposal["packages"] = {"a": {"version": "1.0.0", "notes": "", "previous_version": "1.0.0",
                                               "semver_check": "unknown", "breaking_changes": ""}}
        self.assertEqual(self.run_prepare()["status"], "noop")
        self.assertEqual(self.writes, [])

    def test_wrong_artifact_binding(self):
        self.artifact["id"] = 9
        self.rejects("prepare_artifact_binding")

    def test_wrong_run_actor_or_source(self):
        for field in ("source_sha", "workflow_sha", "run_id", "run_attempt", "actor", "source_tree", "base"):
            with self.subTest(field=field):
                original = self.proposal[field]
                self.proposal[field] = "wrong"
                self.rejects("prepare_proposal_identity")
                self.proposal[field] = original

    def test_tree_truncation(self):
        self.truncated = True
        self.rejects("prepare_tree_incomplete")

    def test_bot_pr_login_identity(self):
        self.run_prepare()
        self.writes = []
        self.pr["user"]["login"] = "other-bot"
        self.rejects("prepare_existing_pr_identity")

    def test_source_files_are_forbidden(self):
        self.proposal["files"].update(self.files({"crates/a/src/lib.rs": b'evil'}))
        self.rejects("prepare_proposal_path")

    def test_root_and_unselected_changelogs_are_forbidden(self):
        for path in ("CHANGELOG.md", "crates/b/CHANGELOG.md"):
            with self.subTest(path=path):
                self.proposal["files"] = self.files({path: b'notes'})
                self.rejects("prepare_proposal_path")

    def test_version_only_manifest_changes(self):
        self.proposal["files"].update(self.files({"crates/a/Cargo.toml":
            self.after["crates/a/Cargo.toml"] + b'[features]\nmalicious = []\n'}))
        self.rejects("proposal_nonversion_edit")

    def test_before_digest_and_package_version(self):
        self.proposal["files"]["crates/a/Cargo.toml"]["before"] = "f" * 40
        self.rejects("prepare_before_changed")
        self.proposal["files"] = self.files(self.after)
        self.proposal["packages"]["a"]["version"] = "99.0.0"
        self.rejects("prepare_package_version")

    def test_notes_must_equal_changed_changelog(self):
        self.proposal["packages"]["a"]["notes"] = "forged notes"
        self.rejects("prepare_package_notes")

    def test_existing_branch_requires_owned_pr(self):
        self.run_prepare()
        self.writes = []
        self.pr["user"]["id"] = 99
        self.rejects("prepare_existing_pr_identity")

    def test_existing_tree_and_parent_are_verified(self):
        self.run_prepare()
        self.writes = []
        self.expected["crates/a/src/lib.rs"]["sha"] = "f" * 40
        self.rejects("prepare_existing_tree")

    def test_exact_orphan_branch_can_create_owned_pr(self):
        self.run_prepare()
        self.pr = None
        self.writes = []
        self.assertEqual(self.run_prepare()["pull_request"], 10)
        self.assertEqual([path for path, _ in self.writes], ["repos/owner/repo/pulls"])
        self.assertEqual(self.receipts[-1]["phase"], "create-pull-request")

    def test_unexpected_orphan_branch_is_rejected(self):
        self.run_prepare()
        self.pr = None
        self.writes = []
        self.expected["crates/a/src/lib.rs"]["sha"] = "f" * 40
        self.rejects("prepare_existing_tree")

    def test_failure_receipt_retains_recoverable_branch_phase(self):
        original = self.scope["prepare_api"]
        def failing(endpoint, payload=None, method="POST"):
            if endpoint.endswith("/pulls"):
                raise self.scope["ReconcileError"]("prepare_api_status_503")
            return original(endpoint, payload, method)
        self.scope["prepare_api"] = failing
        self.scope["policy"] = lambda: self.approved
        with patch.dict(os.environ, self.env, clear=True):
            with self.assertRaises(SystemExit) as failure:
                self.scope["preparation_forge_main"]()
            self.assertEqual(failure.exception.code, 1)
        self.assertEqual(self.receipts[-1]["status"], "failed")
        self.assertEqual(self.receipts[-1]["phase"], "create-pull-request")
        self.assertEqual(self.receipts[-1]["head_sha"], self.head)

    def test_unselected_package_version_is_forbidden(self):
        self.proposal["files"].update(self.files({"crates/b/Cargo.toml":
            self.after["crates/b/Cargo.toml"].replace(b'version = "1.0.0"', b'version = "1.1.0"')}))
        self.rejects("proposal_nonversion_edit")

    def test_workspace_version_cannot_implicitly_bump_unselected_package(self):
        self.before["Cargo.toml"] += b'[workspace.package]\nversion = "1.0.0"\n'
        for name in ("a", "b"):
            path = f"crates/{name}/Cargo.toml"
            self.before[path] = f'[package]\nname = "{name}"\nversion.workspace = true\n'.encode()
        self.blobs = {self.blob_sha(raw): raw for raw in self.before.values()}
        self.entries = {path: {"sha": self.blob_sha(raw), "mode": "100644", "type": "blob"}
                        for path, raw in self.before.items()}
        self.proposal["files"] = self.files({
            "Cargo.toml": self.before["Cargo.toml"].replace(b'1.0.0', b'1.1.0'),
            "crates/a/CHANGELOG.md": self.after["crates/a/CHANGELOG.md"]})
        self.rejects("prepare_unselected_package_version")

    def test_schema_boolean_is_not_integer_one(self):
        self.proposal["schema"] = True
        self.rejects("prepare_proposal_schema")

    def test_lock_version_must_match_after_manifest(self):
        self.proposal["files"].update(self.files({"Cargo.lock":
            self.after["Cargo.lock"].replace(b'1.1.0', b'9.9.9')}))
        self.rejects("proposal_lock_package_version")

    def test_typed_semver_status_and_breaking_report(self):
        details = self.proposal["packages"]["a"]
        details["semver_check"] = "invalid"
        self.rejects("prepare_proposal_package")
        details["semver_check"] = "incompatible"
        self.rejects("prepare_proposal_package")
        details["breaking_changes"] = "Removed public API."
        details["previous_version"] = "0.9.0"
        self.run_prepare()
        self.assertIn("0.9.0 to 1.1.0", self.pr["body"])
        self.assertIn("Semver check: incompatible.", self.pr["body"])
        self.assertIn("### Breaking changes\n\nRemoved public API.", self.pr["body"])

    def test_only_updated_packages_in_body_and_title(self):
        approved = copy.deepcopy(self.approved)
        approved["packages"]["b"] = "1.0.0"
        proposal = copy.deepcopy(self.proposal)
        proposal["packages"]["b"] = {"version": "1.0.0", "notes": "", "previous_version": "1.0.0",
                                    "semver_check": "unknown", "breaking_changes": ""}
        title, body = self.scope["prepare_pr_body"](approved, proposal, True)
        self.assertEqual(title, "chore(a): release v1.1.0")
        self.assertNotIn("## b:", body)
        title, _ = self.scope["prepare_pr_body"](approved, proposal, False)
        self.assertEqual(title, "chore: release v1.1.0")

    def test_upstream_publishability_examples_and_registry_override(self):
        publishable = self.scope["prepare_publishable"]
        example = {"package": {"name": "example"}, "example": [{"name": "demo"}]}
        self.assertFalse(publishable(example, {}, "examples/demo/Cargo.toml", {}))
        example["package"]["publish"] = ["custom-registry"]
        self.assertTrue(publishable(example, {}, "examples/demo/Cargo.toml", {}))
        example["package"]["publish"] = False
        self.assertFalse(publishable(example, {}, "examples/demo/Cargo.toml", {}))
        example["package"].pop("publish")
        self.assertTrue(publishable(example, {}, "examples/demo/Cargo.toml",
                                    {"examples/demo/src/lib.rs": {}}))

    def test_virtual_root_workspace_dependency_binds_next_version(self):
        self.before["Cargo.toml"] += b'[workspace.dependencies]\na = { path = "crates/a", version = "1.0.0" }\n'
        self.blobs = {self.blob_sha(raw): raw for raw in self.before.values()}
        self.entries = {path: {"sha": self.blob_sha(raw), "mode": "100644", "type": "blob"}
                        for path, raw in self.before.items()}
        root_after = self.before["Cargo.toml"].replace(b'1.0.0', b'1.1.0')
        self.after["Cargo.toml"] = root_after
        self.proposal["files"] = self.files(self.after)
        self.run_prepare()
        self.writes = []
        self.proposal["files"].update(self.files({"Cargo.toml": root_after.replace(b'1.1.0', b'9.9.9')}))
        self.rejects("proposal_dependency_version")

    def set_root_source(self, raw):
        self.before["Cargo.toml"] = raw
        self.blobs = {self.blob_sha(content): content for content in self.before.values()}
        self.entries = {path: {"sha": self.blob_sha(content), "mode": "100644", "type": "blob"}
                        for path, content in self.before.items()}
        self.proposal["files"] = self.files(self.after)







    def test_artifact_workspace_mapping_must_equal_source_scope(self):
        self.proposal["manifests"]["a"] = "untrusted/Cargo.toml"
        self.rejects("prepare_proposal_workspace")





    def test_nested_policy_boolean_does_not_equal_integer(self):
        self.approved["schema"] = 1
        self.proposal["policy"] = dict(self.approved, schema=True)
        self.rejects("prepare_proposal_identity")

    def test_workspace_inheritance_requires_boolean_true(self):
        root = self.before["Cargo.toml"] + b'[workspace.package]\nversion = "1.0.0"\n'
        self.before["crates/a/Cargo.toml"] = b'[package]\nname = "a"\nversion.workspace = true\n'
        self.after["crates/a/Cargo.toml"] = b'[package]\nname = "a"\nversion.workspace = 1\n'
        self.after["Cargo.toml"] = root.replace(b'1.0.0', b'1.1.0')
        self.set_root_source(root)
        self.rejects("proposal_nonversion_edit|prepare_package_version")

    def test_artifact_timeout_and_bad_zip_have_durable_failure_receipts(self):
        self.scope["PREPARATION_PROGRESS"] = None
        for error in (subprocess.TimeoutExpired("gh", 40), zipfile.BadZipFile("bad zip")):
            with self.subTest(error=type(error).__name__):
                def failing(error=error):
                    raise error
                self.scope["prepare_pull_request"] = failing
                with self.assertRaises(SystemExit) as failure:
                    self.scope["preparation_forge_main"]()
                self.assertEqual(failure.exception.code, 1)
                self.assertEqual(self.receipts[-1], {"status": "failed", "reason": type(error).__name__})
                self.assertEqual(self.writes, [])


if __name__ == "__main__":
    unittest.main()
