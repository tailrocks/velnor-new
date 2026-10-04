"""Fresh forge preflight tests with authenticated ZIP and job fixtures."""

import hashlib
import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile


ROOT = Path(__file__).resolve().parents[1] / "src"
RUST = ROOT.parents[1] / "velnor-actions-rust" / "src"
NAMESPACE = {"__name__": "release_forge_preflight_test"}
SOURCES = (
    (ROOT, "release_reconcile_common.py"),
    (RUST, "release_reconcile_cargo.py"),
    (RUST, "release_package_contract.py"),
    (RUST, "release_reconcile_registry.py"),
    (RUST, "release_publish_metadata.py"),
    (RUST, "release_publish_manifest.py"),
    (RUST, "release_publish_verify.py"),
    (RUST, "release_publish_artifact.py"),
    (ROOT, "release_reconcile_forge.py"),
    (ROOT, "release_publish_proof.py"),
    (ROOT, "release_forge_preflight.py"),
)
for owner, filename in SOURCES:
    path = owner / filename
    exec(compile(path.read_text(), str(path), "exec"), NAMESPACE)

SOURCE_SHA = "a" * 40
WORKFLOW_SHA = "b" * 40
POLICY = {
    "schema": 1,
    "repository": "owner/repo",
    "registry": "crates-io",
    "source_sha": SOURCE_SHA,
    "packages": {"demo": "1.0.0"},
    "owners": {"demo": ["user:1"]},
    "tags": {"demo": "demo-v1.0.0"},
    "authentication": "trusted-publishing",
    "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")},
    "intent_id": "intent-1",
}


def crate_bytes():
    files = {
        "Cargo.toml": b'[package]\nname = "demo"\nversion = "1.0.0"\n',
        "src/lib.rs": b"pub fn fixture() {}\n",
        ".cargo_vcs_info.json": json.dumps(
            {"git": {"sha1": SOURCE_SHA}, "path_in_vcs": ""}
        ).encode(),
    }
    import tarfile

    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for path, content in files.items():
            member = tarfile.TarInfo("demo-1.0.0/" + path)
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
    return output.getvalue()


def package_proof(archive):
    metadata = {key: None for key in (
        "description", "documentation", "homepage", "readme", "readme_file",
        "license", "license_file", "repository", "links", "rust_version",
    )}
    metadata.update(
        name="demo", vers="1.0.0", deps=[], features={}, authors=[],
        keywords=[], categories=[], badges={}
    )
    proof = NAMESPACE["inventory"](archive, "demo", "1.0.0", SOURCE_SHA)
    proof.update(
        archive_sha256=hashlib.sha256(archive).hexdigest(),
        publish_metadata=metadata,
        dependencies=[],
        cargo_dependency_proofs=[],
        forge_release={
            "tag_name": "demo-v1.0.0", "name": "demo 1.0.0",
            "body": "Fixture notes", "draft": False, "prerelease": False,
        },
    )
    return proof


def candidate_bytes(run_attempt="2", archive=None):
    archive = archive or crate_bytes()
    value = {
        "schema": 1,
        "policy": POLICY,
        "packages": {"demo": package_proof(archive)},
        "publication_order": ["demo"],
        "workflow_sha": WORKFLOW_SHA,
        "run_id": "123",
        "run_attempt": run_attempt,
        "status": "package-verified",
    }
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def zip_bytes(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, content in files.items():
            archive.writestr(name, content)
    return output.getvalue()


class ForgeFixture:
    def __init__(self):
        self.archive = crate_bytes()
        self.content = candidate_bytes(archive=self.archive)
        self.blob = zip_bytes({
            "evidence.json": self.content,
            "crates/demo-1.0.0.crate": self.archive,
        })
        self.artifact = {
            "id": 42,
            "name": "velnor-release-package-r123-a2",
            "expired": False,
            "size_in_bytes": len(self.blob),
            "digest": "sha256:" + hashlib.sha256(self.blob).hexdigest(),
            "workflow_run": {
                "id": 123,
                "run_attempt": 2,
                "head_sha": WORKFLOW_SHA,
                "repository_id": 1,
                "head_repository_id": 1,
            },
        }

    def api(self, endpoint):
        if endpoint.endswith("attempts/2"):
            return {
                "id": 123,
                "run_attempt": 2,
                "head_sha": WORKFLOW_SHA,
                "repository": {"full_name": "owner/repo", "id": 1},
                "head_repository": {"full_name": "owner/repo", "id": 1},
                "workflow_id": 9,
            }
        if endpoint.endswith("workflows/9"):
            return {"path": ".github/workflows/release.yml"}
        if endpoint.endswith("/jobs?per_page=100&page=1"):
            return {"jobs": [{
                "id": 77,
                "name": "release-package",
                "run_id": 123,
                "run_attempt": 2,
                "head_sha": WORKFLOW_SHA,
                "status": "completed",
                "conclusion": "success",
            }]}
        if "/artifacts?" in endpoint:
            return {"artifacts": [self.artifact]}
        raise AssertionError(endpoint)


class ForgePreflightTests(unittest.TestCase):
    def setUp(self):
        self.fixture = ForgeFixture()
        self.environment = {
            "GITHUB_SHA": WORKFLOW_SHA,
            "GITHUB_RUN_ID": "123",
            "GITHUB_RUN_ATTEMPT": "2",
            "RELEASE_PACKAGE_ARTIFACT_ID": "42",
            "RELEASE_PACKAGE_ARTIFACT_DIGEST": self.fixture.artifact["digest"].removeprefix("sha256:"),
        }

    def test_authenticated_candidate_binds_job_attempt_and_raw_digest_before_parse(self):
        calls = []

        def download(argv, **kwargs):
            calls.append(argv)
            return SimpleNamespace(returncode=0, stdout=self.fixture.blob)

        with patch.dict(os.environ, self.environment), \
                patch.dict(NAMESPACE, {"forge_api": self.fixture.api}), \
                patch.object(NAMESPACE["subprocess"], "run", side_effect=download):
            content, archives, receipt, blob = NAMESPACE["load_package_input"](POLICY)
        self.assertEqual(content, json.loads(self.fixture.content))
        self.assertEqual(archives, {"demo": self.fixture.archive})
        self.assertEqual(receipt["producer_job"], {
            "id": 77, "name": "release-package", "conclusion": "success",
        })
        self.assertEqual(blob, self.fixture.blob)
        self.assertEqual(calls[0][0], "gh")

    def test_digest_failure_happens_before_zip_parse(self):
        self.fixture.blob = b"tampered raw ZIP bytes"
        with patch.dict(os.environ, self.environment), \
                patch.dict(NAMESPACE, {"forge_api": self.fixture.api}), \
                patch.object(NAMESPACE["subprocess"], "run",
                             return_value=SimpleNamespace(returncode=0, stdout=self.fixture.blob)), \
                patch.object(NAMESPACE["zipfile"], "ZipFile", side_effect=AssertionError("parsed")):
            with self.assertRaisesRegex(NAMESPACE["ReconcileError"], "artifact_digest_mismatch"):
                NAMESPACE["load_package_input"](POLICY)

    def test_producer_attempt_mismatch_is_rejected(self):
        def api(endpoint):
            value = self.fixture.api(endpoint)
            if "jobs?" in endpoint:
                value["jobs"][0]["run_attempt"] = 1
            return value
        with patch.dict(os.environ, self.environment), patch.dict(NAMESPACE, {"forge_api": api}), \
                patch.object(NAMESPACE["subprocess"], "run",
                             return_value=SimpleNamespace(returncode=0, stdout=self.fixture.blob)):
            with self.assertRaisesRegex(NAMESPACE["ReconcileError"], "artifact_producer_identity"):
                NAMESPACE["load_package_input"](POLICY)

    def test_wrong_upload_id_or_digest_rejects_before_download(self):
        for key, value in [("RELEASE_PACKAGE_ARTIFACT_ID", "43"),
                           ("RELEASE_PACKAGE_ARTIFACT_DIGEST", "0" * 64)]:
            with patch.dict(os.environ, {**self.environment, key: value}), \
                    patch.dict(NAMESPACE, {"forge_api": self.fixture.api}), \
                    patch.object(NAMESPACE["subprocess"], "run", side_effect=AssertionError("downloaded")):
                with self.assertRaisesRegex(NAMESPACE["ReconcileError"], "artifact_upload_binding_mismatch"):
                    NAMESPACE["load_package_input"](POLICY)

    def test_forge_preflight_writes_only_final_publication_evidence(self):
        observed = {}

        def package_input(approved):
            observed["input"] = approved
            return (
                json.loads(self.fixture.content),
                {"demo": self.fixture.archive},
                {"id": 42, "digest": self.fixture.artifact["digest"],
                 "name": "velnor-release-package-r123-a2",
                 "producer_job": {"id": 77, "name": "release-package",
                                  "conclusion": "success"}},
                self.fixture.blob,
            )

        def forge(approved, name, require_release):
            observed.setdefault("forge", []).append((name, require_release))
            return {"status": "absent"}

        with self._temporary_working_directory() as directory, patch.dict(
                os.environ, {**self.environment, "RELEASE_RECONCILE_POLICY": json.dumps(POLICY)}
        ), \
                patch.dict(NAMESPACE, {
                    "load_package_input": package_input,
                    "forge_package": forge,
                    "fetch": lambda *_: None,
                }):
            NAMESPACE["create_forge_preflight"]()
            evidence = json.loads((directory / "release-preflight/evidence.json").read_text())
        self.assertEqual(observed["input"], POLICY)
        self.assertEqual(evidence["status"], "publication-incomplete")
        self.assertEqual(evidence["publication_order"], ["demo"])
        self.assertEqual(evidence["operations"], {
            "demo": {"version": "1.0.0", "status": "pending"}
        })
        self.assertEqual(set(evidence), {
            "schema", "policy", "packages", "publication_order", "workflow_sha",
            "run_id", "run_attempt", "status", "operations"
        })

    def test_forge_source_has_no_repository_task_or_config_execution(self):
        source = (ROOT / "release_forge_preflight.py").read_text()
        self.assertNotIn('"cargo"', source)
        self.assertNotIn('"git"', source)
        self.assertNotIn("--config", source)

    @staticmethod
    def _temporary_working_directory():
        from contextlib import contextmanager
        import tempfile

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
