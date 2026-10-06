"""Mocked checkout-free forge publication proofs; never contact GitHub."""

import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1] / "src"
RUST = ROOT.parent.parent / "velnor-actions-rust" / "src"
NAMESPACE = {"__name__": "release_forge_publish_test"}
for filename in ("release_reconcile_common.py", "release_reconcile_forge.py",
                 "release_package_contract.py",
                 "release_forge_publish_read.py", "release_forge_publish_verify.py",
                 "release_forge_publish_api.py", "release_publish_verify.py",
                 "release_publish_proof.py",
                 "release_forge_publish.py"):
    source = RUST if filename in {
        "release_package_contract.py", "release_publish_verify.py",
    } else ROOT
    exec(compile((source / filename).read_text(), filename, "exec"), NAMESPACE)

SOURCE = "a" * 40
WORKFLOW = "b" * 40
APPROVED = {
    "schema": 1,
    "repository": "owner/repo",
    "registry": "crates-io",
    "source_sha": SOURCE,
    "packages": {"demo": "1.0.0"},
    "owners": {"demo": ["user:1"]},
    "tags": {"demo": "demo-v1.0.0"},
    "authentication": "trusted-publishing",
    "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")},
    "intent_id": "intent-1",
}
DESCRIPTOR = {
    "tag_name": "demo-v1.0.0",
    "body": "Release notes.",
    "name": "demo-v1.0.0",
    "draft": False,
    "prerelease": False,
}


def candidate(descriptor=DESCRIPTOR, package=None):
    if package is None:
        package = {"forge_release": descriptor}
    return {
        "schema": 1,
        "policy": APPROVED,
        "packages": {"demo": package},
        "publication_order": ["demo"],
        "workflow_sha": WORKFLOW,
        "run_id": "123",
        "run_attempt": "2",
        "status": "package-verified",
    }


def proofs():
    return {
        "package_artifact": {
            "id": 11, "digest": "sha256:" + "1" * 64,
            "name": "velnor-release-package-r123-a2",
            "producer_job": {"id": 101, "name": "release-package", "conclusion": "success"},
        },
        "registry_artifact": {
            "id": 12, "digest": "sha256:" + "2" * 64,
            "name": "velnor-release-registry-r123-a2",
            "producer_job": {
                "id": 102, "name": "release-registry-publish", "conclusion": "success",
            },
        },
    }


class Remote:
    def __init__(self):
        self.tag_object = None
        self.ref = None
        self.release = None
        self.created = []

    def read_ref(self, _repository, _tag):
        return self.ref

    def read_tag(self, _repository, sha):
        return self.tag_object if self.tag_object and self.tag_object["sha"] == sha else None

    def read_release(self, _repository, _tag):
        return self.release

    def create_tag(self, _repository, payload):
        self.created.append(("tag", payload))
        self.tag_object = {
            "sha": "c" * 40, "tag": payload["tag"], "message": payload["message"],
            "object": {"sha": payload["object"], "type": "commit"},
        }
        return self.tag_object

    def create_ref(self, _repository, payload):
        self.created.append(("ref", payload))
        self.ref = {"ref": payload["ref"],
                    "object": {"sha": payload["sha"], "type": "tag"}}
        return self.ref

    def create_release(self, _repository, payload):
        self.created.append(("release", payload))
        self.release = {
            **payload,
            "html_url": "https://github.com/owner/repo/releases/tag/demo-v1.0.0",
        }
        return self.release


class ForgePublishTests(unittest.TestCase):
    def setUp(self):
        self.environment = {"GITHUB_SHA": WORKFLOW, "GITHUB_RUN_ID": "123",
                            "GITHUB_RUN_ATTEMPT": "2", "GH_TOKEN": "token",
                            "RELEASE_PACKAGE_ARTIFACT_ID": "11",
                            "RELEASE_PACKAGE_ARTIFACT_DIGEST": "1" * 64,
                            "RELEASE_REGISTRY_RECEIPT_ARTIFACT_ID": "12",
                            "RELEASE_REGISTRY_RECEIPT_ARTIFACT_DIGEST": "2" * 64}

    def run_publish(self, remote, value=None, overrides=None):
        loaded = (candidate() if value is None else value, proofs())
        bindings = {
            "load_forge_publish_input": lambda _approved: loaded,
            "read_tag_ref": remote.read_ref,
            "read_tag_object": remote.read_tag,
            "read_release": remote.read_release,
            "create_tag": remote.create_tag,
            "create_tag_ref": remote.create_ref,
            "create_release": remote.create_release,
        }
        if overrides:
            bindings.update(overrides)
        with tempfile.TemporaryDirectory() as directory, \
                patch.dict(os.environ, self.environment, clear=True), \
                patch.dict(NAMESPACE, bindings):
            previous = Path.cwd()
            os.chdir(directory)
            try:
                return NAMESPACE["publish_forge"](APPROVED)
            finally:
                os.chdir(previous)

    def test_new_annotated_tag_then_exact_release(self):
        remote = Remote()
        receipt = self.run_publish(remote)
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(set(receipt), {
            "schema", "policy", "status", "workflow_sha", "run_id", "run_attempt",
            "publication_order", "operations",
        })
        operation = receipt["operations"]["demo"]
        self.assertEqual(operation["status"], "verified")
        self.assertEqual(operation["tag"]["object_sha"], "c" * 40)
        self.assertEqual(
            operation["release"]["html_url"],
            "https://github.com/owner/repo/releases/tag/demo-v1.0.0",
        )
        self.assertEqual([kind for kind, _ in remote.created], ["tag", "ref", "release"])
        self.assertEqual(remote.created[0][1], {
            "tag": "demo-v1.0.0", "message": "chore: Release package demo version 1.0.0",
            "object": SOURCE, "type": "commit",
        })
        self.assertNotIn("make_latest", remote.created[2][1])
        self.assertNotIn("generate_release_notes", remote.created[2][1])

    def test_existing_exact_tag_and_release_are_reused_without_writes(self):
        remote = Remote()
        remote.tag_object = {
            "sha": "c" * 40, "tag": "demo-v1.0.0",
            "message": "chore: Release package demo version 1.0.0",
            "object": {"sha": SOURCE, "type": "commit"},
        }
        remote.ref = {"ref": "refs/tags/demo-v1.0.0",
                      "object": {"sha": "c" * 40, "type": "tag"}}
        remote.release = {**DESCRIPTOR, "html_url":
                          "https://github.com/owner/repo/releases/tag/demo-v1.0.0"}
        receipt = self.run_publish(remote)
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(remote.created, [])

    def test_mismatched_existing_annotated_tag_is_collision(self):
        remote = Remote()
        remote.tag_object = {
            "sha": "c" * 40, "tag": "demo-v1.0.0", "message": "wrong",
            "object": {"sha": SOURCE, "type": "commit"},
        }
        remote.ref = {"ref": "refs/tags/demo-v1.0.0",
                      "object": {"sha": "c" * 40, "type": "tag"}}
        receipt = self.run_publish(remote)
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(receipt["operations"]["demo"]["status"], "failed")
        self.assertEqual(remote.created, [])

    def test_release_ambiguous_write_recovers_with_bounded_retry(self):
        remote = Remote()
        calls = 0

        def uncertain_once(repository, payload):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise NAMESPACE["ForgeWriteUncertain"]("transport")
            return remote.create_release(repository, payload)

        receipt = self.run_publish(remote, overrides={"create_release": uncertain_once})
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(calls, 2)

    def test_release_failure_keeps_verified_tag_in_incomplete_receipt(self):
        remote = Remote()

        def uncertain(_repository, _payload):
            raise NAMESPACE["ForgeWriteUncertain"]("transport")

        receipt = self.run_publish(remote, overrides={"create_release": uncertain})
        operation = receipt["operations"]["demo"]
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(operation["status"], "failed")
        self.assertEqual(operation["tag"]["source_sha"], SOURCE)
        self.assertNotIn("release", operation)
        self.assertEqual([kind for kind, _ in remote.created], ["tag", "ref"])

    def test_invalid_descriptor_is_rejected_before_tag_write(self):
        remote = Remote()
        bad = {**DESCRIPTOR, "make_latest": True}
        receipt = self.run_publish(remote, candidate(bad))
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(remote.created, [])

    def test_prerelease_flag_must_match_approved_version(self):
        remote = Remote()
        bad = {**DESCRIPTOR, "prerelease": True}
        receipt = self.run_publish(remote, candidate(bad))
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(remote.created, [])

    def test_explicit_empty_notes_are_published_verbatim(self):
        remote = Remote()
        empty_notes = {**DESCRIPTOR, "body": ""}
        receipt = self.run_publish(remote, candidate(empty_notes))
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["operations"]["demo"]["release"]["body"], "")

    def test_readonly_verification_checks_exact_tag_and_release(self):
        remote = Remote()
        remote.tag_object = {
            "sha": "c" * 40, "tag": "demo-v1.0.0",
            "message": "chore: Release package demo version 1.0.0",
            "object": {"sha": SOURCE, "type": "commit"},
        }
        remote.ref = {
            "ref": "refs/tags/demo-v1.0.0",
            "object": {"sha": "c" * 40, "type": "tag"},
        }
        remote.release = {
            **DESCRIPTOR,
            "html_url": "https://github.com/owner/repo/releases/tag/demo-v1.0.0",
        }
        with patch.dict(os.environ, self.environment, clear=True), patch.dict(NAMESPACE, {
            "read_tag_ref": remote.read_ref,
            "read_tag_object": remote.read_tag,
            "read_release": remote.read_release,
        }):
            result = NAMESPACE["verify_forge_package"](APPROVED, "demo", DESCRIPTOR)
        self.assertEqual(result, {
            "status": "verified", "tag": "demo-v1.0.0", "target": "c" * 40,
            "release_url": "https://github.com/owner/repo/releases/tag/demo-v1.0.0",
        })
        self.assertEqual(remote.created, [])

    def test_readonly_verification_rejects_mismatched_tag_object_sha(self):
        remote = Remote()
        remote.tag_object = {
            "sha": "c" * 40, "tag": "demo-v1.0.0",
            "message": "chore: Release package demo version 1.0.0",
            "object": {"sha": SOURCE, "type": "commit"},
        }
        remote.ref = {
            "ref": "refs/tags/demo-v1.0.0",
            "object": {"sha": "c" * 40, "type": "tag"},
        }
        mismatched = {**remote.tag_object, "sha": "d" * 40}
        with patch.dict(os.environ, self.environment, clear=True), patch.dict(NAMESPACE, {
            "read_tag_ref": remote.read_ref,
            "read_tag_object": lambda *_args: mismatched,
            "read_release": remote.read_release,
        }), self.assertRaisesRegex(NAMESPACE["ReconcileError"], "forge_tag_proof"):
            NAMESPACE["verify_forge_package"](APPROVED, "demo", DESCRIPTOR)

    def test_actual_loader_binds_candidate_and_both_artifact_proofs(self):
        package_artifact, registry_artifact = proofs().values()
        package_files = {"Cargo.toml": {"sha256": "e" * 64, "size": 1}}
        package_value = {
            "files": package_files, "features": {}, "archive_sha256": "d" * 64,
            "publish_metadata": {"vers": "1.0.0"}, "dependencies": [],
            "cargo_dependency_proofs": [], "forge_release": DESCRIPTOR,
        }
        registry_proof = {
            "status": "verified", "registry_checksum": "d" * 64,
            "archive_checksum": "d" * 64, "owners": APPROVED["owners"]["demo"],
            "files": package_files, "features": {},
        }
        registry_receipt = {
            "schema": 1, "policy": APPROVED, "status": "verified",
            "workflow_sha": WORKFLOW, "run_id": "123", "run_attempt": "2",
            "publication_order": ["demo"], "operations": {"demo": {
                "version": "1.0.0", "status": "verified",
                "candidate_archive_sha256": "d" * 64,
                "relation": "existing-normalized", "registry": registry_proof,
            }},
        }
        candidate_value = candidate(package=package_value)

        def remote_registry_proof(approved, name, version, expected):
            self.assertIs(approved, APPROVED)
            self.assertEqual((name, version), ("demo", "1.0.0"))
            self.assertIs(expected, package_value)
            return registry_proof

        with patch.dict(os.environ, self.environment, clear=True), patch.dict(NAMESPACE, {
            "load_package_input": lambda _approved: (
                candidate_value, {}, package_artifact, b"zip"
            ),
            "load_publish_receipt": lambda _approved, _kind: (registry_receipt, registry_artifact),
            "verify_published_package": remote_registry_proof,
        }):
            loaded = NAMESPACE["load_forge_publish_input"](APPROVED)
            value, order = NAMESPACE["_candidate_values"](APPROVED, loaded)
        self.assertEqual(order, ["demo"])
        self.assertEqual(value["packages"]["demo"]["forge_release"], DESCRIPTOR)
        self.assertIs(value["packages"]["demo"], package_value)
        self.assertEqual(loaded[1]["registry_artifact"], registry_artifact)


class ForgeApiTests(unittest.TestCase):
    def test_read_allowlist_and_redirect_denial(self):
        response = type("Response", (), {
            "status": 200,
            "read": lambda self, _limit: b'{"ref":"refs/tags/demo-v1.0.0"}',
            "__enter__": lambda self: self,
            "__exit__": lambda self, *_args: False,
        })()
        class Opener:
            def open(self, request, timeout):
                self.request, self.timeout = request, timeout
                return response

        opener = Opener()
        with patch.dict(os.environ, {"GH_TOKEN": "token"}, clear=True), \
                patch.object(NAMESPACE["urllib"].request, "build_opener", return_value=opener):
            result = NAMESPACE["forge_read_request"](
                "owner/repo", "repos/owner/repo/git/ref/tags/demo-v1.0.0"
            )
        self.assertEqual(result["ref"], "refs/tags/demo-v1.0.0")
        self.assertEqual(
            opener.request.full_url,
            "https://api.github.com/repos/owner/repo/git/ref/tags/demo-v1.0.0",
        )
        self.assertEqual(opener.request.get_header("Authorization"), "Bearer token")
        with self.assertRaisesRegex(NAMESPACE["ReconcileError"], "forge_publish_method"):
            NAMESPACE["forge_request"]("owner/repo", "DELETE", "repos/owner/repo/releases")

    def test_write_allowlist_requires_exact_post_payload(self):
        response = type("Response", (), {
            "status": 201,
            "read": lambda self, _limit: b'{"sha":"' + b"c" * 40 + b'"}',
            "__enter__": lambda self: self,
            "__exit__": lambda self, *_args: False,
        })()

        class Opener:
            def open(self, request, timeout):
                self.request, self.timeout = request, timeout
                return response

        opener = Opener()
        payload = {
            "tag": "demo-v1.0.0",
            "message": "chore: Release package demo version 1.0.0",
            "object": SOURCE,
            "type": "commit",
        }
        with patch.dict(os.environ, {"GH_TOKEN": "token"}, clear=True), \
                patch.object(NAMESPACE["urllib"].request, "build_opener", return_value=opener):
            result = NAMESPACE["forge_request"](
                "owner/repo", "POST", "repos/owner/repo/git/tags", payload
            )
        self.assertEqual(result["sha"], "c" * 40)
        self.assertEqual(opener.request.get_method(), "POST")
if __name__ == "__main__":
    unittest.main()
