"""Real package ZIP crosses anonymous, registry, and Forge proof boundaries."""
from contextlib import nullcontext
import hashlib
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile


CRATES = Path(__file__).resolve().parents[2]
GENERIC = CRATES / "velnor-actions-orchestrator" / "src"
RUST = CRATES / "velnor-actions-rust" / "src"
NS = {"__name__": "release_publish_boundary_test"}
SOURCES = (
    (GENERIC, "release_reconcile_common.py"),
    (GENERIC, "release_reconcile_forge.py"),
    (RUST, "release_reconcile_cargo.py"),
    (RUST, "release_reconcile_registry.py"),
    (RUST, "release_package_contract.py"),
    (RUST, "release_publish_metadata.py"),
    (RUST, "release_publish_manifest.py"),
    (RUST, "release_publish_verify.py"),
    (RUST, "release_publish_registry.py"),
    (RUST, "release_publish_artifact.py"),
    (RUST, "release_publish_entry.py"),
    (GENERIC, "release_publish_proof.py"),
)
for owner, filename in SOURCES:
    path = owner / filename
    exec(compile(path.read_text(), str(path), "exec"), NS)

SOURCE, WORKFLOW = "a" * 40, "b" * 40
APPROVED = {
    "schema": 1, "repository": "owner/repo", "registry": "crates-io",
    "source_sha": SOURCE, "packages": {"demo": "1.0.0"},
    "owners": {"demo": ["user:1"]}, "tags": {"demo": "demo-v1.0.0"},
    "authentication": "trusted-publishing", "intent_id": "intent-1",
    "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")},
}


def crate_bytes(alternate=False):
    manifest = b'[package]\nname="demo"\nversion="1.0.0"\n'
    if alternate:
        manifest = b'[package]\nversion = "1.0.0"\nname = "demo"\n'
    files = {"Cargo.toml": manifest, "src/lib.rs": b"pub fn fixture() {}\n",
             ".cargo_vcs_info.json": json.dumps({"git": {"sha1": SOURCE},
                                                "path_in_vcs": ""}).encode()}
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for path, content in files.items():
            member = tarfile.TarInfo("demo-1.0.0/" + path)
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
    return output.getvalue()


def package_proof(blob):
    metadata = {key: None for key in ("description", "documentation", "homepage", "readme",
                "readme_file", "license", "license_file", "repository", "links", "rust_version")}
    metadata.update(name="demo", vers="1.0.0", deps=[], features={}, authors=[], keywords=[],
                    categories=[], badges={})
    proof = NS["inventory"](blob, "demo", "1.0.0", SOURCE)
    proof.update(archive_sha256=hashlib.sha256(blob).hexdigest(), publish_metadata=metadata,
                 dependencies=[], cargo_dependency_proofs=[], forge_release={
                     "tag_name": "demo-v1.0.0", "name": "demo 1.0.0", "body": "Fixture notes",
                     "draft": False, "prerelease": False})
    return proof


def zip_bytes(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, content in files.items():
            archive.writestr(name, content)
    return output.getvalue()


class BoundaryRemote:
    """Only external observations are replaced; every proof parser stays real."""
    def __init__(self, package_zip):
        self.blobs = {11: package_zip}
        self.producers = {11: "release-package", 12: "release-registry-publish"}
        self.conclusions = {11: "success", 12: "success"}
        self.artifact_overrides = {}
        self.job_overrides = {}

    def artifact(self, identifier):
        prefix = "package" if identifier == 11 else "registry"
        return {"id": identifier, "name": f"velnor-release-{prefix}-r123-a2",
                "size_in_bytes": len(self.blobs[identifier]), "expired": False,
                "digest": "sha256:" + hashlib.sha256(self.blobs[identifier]).hexdigest(),
                "workflow_run": {"id": 123, "head_sha": WORKFLOW, "repository_id": 7,
                                 "head_repository_id": 7}, **self.artifact_overrides}

    def api(self, endpoint):
        if endpoint.endswith("/attempts/2"):
            return {"id": 123, "run_attempt": 2, "head_sha": WORKFLOW, "workflow_id": 9,
                    "repository": {"id": 7, "full_name": "owner/repo"},
                    "head_repository": {"id": 7, "full_name": "owner/repo"}}
        if endpoint.endswith("/workflows/9"):
            return {"path": ".github/workflows/release.yml"}
        if "/jobs?" in endpoint:
            return {"jobs": [{"id": 100 + identifier, "name": self.producers[identifier],
                              "run_id": 123, "run_attempt": 2, "head_sha": WORKFLOW,
                              "status": "completed", "conclusion": self.conclusions[identifier],
                              **self.job_overrides} for identifier in self.blobs]}
        if "/artifacts?" in endpoint:
            return {"artifacts": [self.artifact(identifier) for identifier in self.blobs]}
        raise AssertionError(endpoint)

    def download(self, argv, **_kwargs):
        identifier = int(argv[-1].split("/")[-2])
        return SimpleNamespace(returncode=0, stdout=self.blobs[identifier])

    def bindings(self):
        environment = {}
        for identifier in self.blobs:
            prefix = "RELEASE_PACKAGE_ARTIFACT" if identifier == 11 else "RELEASE_REGISTRY_RECEIPT_ARTIFACT"
            environment[prefix + "_ID"] = str(identifier)
            environment[prefix + "_DIGEST"] = hashlib.sha256(self.blobs[identifier]).hexdigest()
        return environment


class PublishBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.archive = crate_bytes()
        self.proof = package_proof(self.archive)
        self.candidate = {"schema": 1, "policy": APPROVED, "packages": {"demo": self.proof},
                          "workflow_sha": WORKFLOW, "run_id": "123", "run_attempt": "2",
                          "status": "package-verified", "publication_order": ["demo"]}
        self.remote = BoundaryRemote(zip_bytes({"evidence.json": json.dumps(self.candidate),
                                               "crates/demo-1.0.0.crate": self.archive}))
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        environment = {"GITHUB_SHA": WORKFLOW, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
                       "RUNNER_TEMP": str(Path(self.temporary.name).resolve()),
                       "RELEASE_RECONCILE_POLICY": json.dumps(APPROVED), **self.remote.bindings()}
        self.environment = patch.dict(os.environ, environment, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.observations = patch.dict(NS, {"forge_api": self.remote.api, "fetch": self.registry_fetch})
        self.observations.start()
        self.addCleanup(self.observations.stop)
        self.download = patch.object(NS["subprocess"], "run", self.remote.download)
        self.network = patch("urllib.request.build_opener", side_effect=AssertionError("network_forbidden"))
        self.network.start()
        self.addCleanup(self.network.stop)
        self.download.start()
        self.addCleanup(self.download.stop)

    def registry_fetch(self, url, _limit=None):
        checksum = hashlib.sha256(self.registry_archive).hexdigest()
        if url.endswith("/owner_user"):
            value = {"users": [{"id": 1}]}
        elif url.endswith("/owner_team"):
            value = {"teams": []}
        elif url.endswith("/dependencies"):
            value = {"dependencies": []}
        elif url.startswith("https://index.crates.io/"):
            value = {"name": "demo", "vers": "1.0.0", "yanked": False,
                     "cksum": checksum, "features": {}, "deps": []}
        elif url.startswith("https://static.crates.io/"):
            return self.registry_archive
        elif url.endswith("/demo/1.0.0"):
            if not self.version_present:
                return None
            value = {"version": {"crate": "demo", "num": "1.0.0", "yanked": False,
                                 "checksum": checksum, "features": {}}}
        elif url.endswith("/demo"):
            value = {"crate": {"name": "demo"}}
        else:
            raise AssertionError(url)
        return json.dumps(value).encode()

    def publish(self, existing):
        NS["create_registry_artifact_proof"]()
        blob = NS["_read_verified_artifact"]()
        self.assertEqual(blob, self.remote.blobs[11])
        candidate, archives = NS["validate_package_artifact"](blob, APPROVED)
        self.registry_archive = crate_bytes(alternate=existing)
        self.version_present = existing
        uploads = []

        def upload(metadata, archive, token):
            self.assertEqual(metadata, self.proof["publish_metadata"])
            self.assertEqual(token, "fixture")
            uploads.append(archive)
            self.registry_archive, self.version_present = archive, True

        previous = Path.cwd()
        os.chdir(self.temporary.name)
        try:
            with patch.dict(NS, {"fetch": self.registry_fetch, "registry_token":
                                 lambda *_args: nullcontext("fixture"), "upload_package": upload}):
                receipt = NS["publish_registry"](APPROVED, candidate["packages"], archives)
        finally:
            os.chdir(previous)
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(len(uploads), 0 if existing else 1)
        self.remote.blobs[12] = zip_bytes({"receipt.json": json.dumps(receipt)})
        os.environ.update(self.remote.bindings())
        loaded, identities = NS["load_forge_publish_input"](APPROVED)
        self.assertEqual(loaded, self.candidate)
        self.assertEqual(identities["package_artifact"]["digest"],
                         "sha256:" + hashlib.sha256(blob).hexdigest())
        self.assertEqual(identities["registry_artifact"]["producer_job"]["id"], 112)
        return receipt

    def test_existing_normalized_crosses_all_boundaries(self):
        receipt = self.publish(existing=True)
        operation = receipt["operations"]["demo"]
        self.assertEqual(operation["relation"], "existing-normalized")
        self.assertNotEqual(operation["registry"]["registry_checksum"], self.proof["archive_sha256"])

    def test_submitted_exact_crosses_all_boundaries(self):
        receipt = self.publish(existing=False)
        operation = receipt["operations"]["demo"]
        self.assertEqual(operation["relation"], "submitted-exact")
        self.assertEqual(operation["registry"]["registry_checksum"], self.proof["archive_sha256"])

    def test_materialized_zip_tamper_rejected(self):
        NS["create_registry_artifact_proof"]()
        root = Path(self.temporary.name) / "velnor" / "verified-release"
        (root / "artifact.zip").chmod(0o600)
        (root / "artifact.zip").write_bytes(self.remote.blobs[11] + b"tamper")
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_verified_digest"):
            NS["_read_verified_artifact"]()

    def test_partial_materialized_producer_rejected(self):
        NS["create_registry_artifact_proof"]()
        root = Path(self.temporary.name) / "velnor" / "verified-release"
        identity = json.loads((root / "artifact.json").read_text())
        del identity["producer_job"]["id"]
        (root / "artifact.json").write_text(json.dumps(identity))
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_proof_producer"):
            NS["_read_verified_artifact"]()

    def test_unknown_relation_rejected_at_forge_boundary(self):
        receipt = self.publish(existing=True)
        receipt["operations"]["demo"]["relation"] = "unproved"
        self.remote.blobs[12] = zip_bytes({"receipt.json": json.dumps(receipt)})
        os.environ.update(self.remote.bindings())
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_operation_relation"):
            NS["load_forge_publish_input"](APPROVED)

    def test_submitted_relation_requires_exact_candidate_bytes(self):
        receipt = self.publish(existing=True)
        receipt["operations"]["demo"]["relation"] = "submitted-exact"
        self.remote.blobs[12] = zip_bytes({"receipt.json": json.dumps(receipt)})
        os.environ.update(self.remote.bindings())
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_operation_submitted_checksum"):
            NS["load_forge_publish_input"](APPROVED)

    def test_failed_registry_producer_cannot_authorize_forge(self):
        self.publish(existing=False)
        self.remote.conclusions[12] = "failure"
        with self.assertRaisesRegex(NS["ReconcileError"], "failed_producer_receipt_status"):
            NS["load_forge_publish_input"](APPROVED)

    def test_boolean_package_schema_rejected(self):
        candidate = {**self.candidate, "schema": True}
        blob = zip_bytes({"evidence.json": json.dumps(candidate),
                          "crates/demo-1.0.0.crate": self.archive})
        with self.assertRaisesRegex(NS["ReconcileError"], "package_artifact_fields"):
            NS["validate_package_artifact"](blob, APPROVED)

    def test_boolean_terminal_schema_rejected(self):
        receipt = self.publish(existing=False)
        receipt["schema"] = True
        self.remote.blobs[12] = zip_bytes({"receipt.json": json.dumps(receipt)})
        os.environ.update(self.remote.bindings())
        with self.assertRaisesRegex(NS["ReconcileError"], "publish_receipt_authority"):
            NS["load_forge_publish_input"](APPROVED)

    def test_boolean_artifact_id_rejected(self):
        self.remote.artifact_overrides["id"] = True
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_identity"):
            NS["load_package_input"](APPROVED)


    def test_direct_upload_digest_binding_rejected(self):
        os.environ["RELEASE_PACKAGE_ARTIFACT_DIGEST"] = "0" * 64
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_upload_binding_mismatch"):
            NS["load_package_input"](APPROVED)

    def test_downloaded_raw_zip_tamper_rejected(self):
        def tampered(argv, **kwargs):
            result = self.remote.download(argv, **kwargs)
            return SimpleNamespace(returncode=0, stdout=result.stdout + b"tamper")
        with patch.object(NS["subprocess"], "run", tampered), self.assertRaisesRegex(
                NS["ReconcileError"], "artifact_digest_mismatch"):
            NS["load_package_input"](APPROVED)

    def test_boolean_producer_id_rejected(self):
        self.remote.job_overrides["id"] = True
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_producer_identity"):
            NS["load_package_input"](APPROVED)

    def test_previous_attempt_producer_rejected(self):
        self.remote.job_overrides["run_attempt"] = 1
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_producer_identity"):
            NS["load_package_input"](APPROVED)

    def test_boolean_policy_schema_rejected(self):
        os.environ["RELEASE_RECONCILE_POLICY"] = json.dumps({**APPROVED, "schema": True})
        with self.assertRaisesRegex(NS["ReconcileError"], "policy_registry"):
            NS["create_registry_artifact_proof"]()

    def test_incomplete_registry_receipt_rejected(self):
        receipt = self.publish(existing=False)
        receipt["status"] = "incomplete"
        receipt["operations"]["demo"] = {"version": "1.0.0", "status": "pending"}
        self.remote.blobs[12] = zip_bytes({"receipt.json": json.dumps(receipt)})
        os.environ.update(self.remote.bindings())
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_publication_incomplete"):
            NS["load_forge_publish_input"](APPROVED)


    def test_nested_candidate_policy_boolean_schema_rejected(self):
        candidate = {**self.candidate, "policy": {**APPROVED, "schema": True}}
        blob = zip_bytes({"evidence.json": json.dumps(candidate),
                          "crates/demo-1.0.0.crate": self.archive})
        with self.assertRaisesRegex(NS["ReconcileError"], "package_artifact_fields"):
            NS["validate_package_artifact"](blob, APPROVED)

    def test_nested_terminal_policy_boolean_schema_rejected(self):
        receipt = self.publish(existing=False)
        receipt["policy"] = {**APPROVED, "schema": True}
        self.remote.blobs[12] = zip_bytes({"receipt.json": json.dumps(receipt)})
        os.environ.update(self.remote.bindings())
        with self.assertRaisesRegex(NS["ReconcileError"], "publish_receipt_authority"):
            NS["load_forge_publish_input"](APPROVED)

    def test_boolean_record_id_cannot_equal_integer_binding(self):
        digest = "sha256:" + "a" * 64
        identity = {"id": True, "digest": digest, "name": "velnor-release-package-r123-a2",
                    "producer_job": {"id": 101, "name": "release-package", "conclusion": "success"}}
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_proof_identity"):
            NS["validate_artifact_identity"](identity, 1, digest, identity["name"],
                                             "release-package", ("success",))


if __name__ == "__main__":
    unittest.main()
