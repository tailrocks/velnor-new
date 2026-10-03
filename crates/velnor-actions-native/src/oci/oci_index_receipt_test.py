"""Isolated immutable OCI index receipt transport tests."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile


def load(name):
    path = Path(__file__).with_name(name + ".py")
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


oci_digest = load("oci_digest")
load("oci_digest_parts")
receipt = load("oci_index_receipt")

SOURCE = "a" * 40
INDEX = "sha256:" + "c" * 64
PLATFORMS = {
    "amd64": "sha256:" + "d" * 64,
    "arm64": "sha256:" + "e" * 64,
}
ENVIRONMENT = {
    "REPOSITORY": "example/repo",
    "GITHUB_REPOSITORY": "example/repo",
    "IMAGE": "registry.example/base",
    "IMAGE_ID": "base",
    "VERSION": "1.2.3",
    "SOURCE_SHA": SOURCE,
    "GITHUB_RUN_ID": "42",
    "GITHUB_RUN_ATTEMPT": "2",
    "GH_TOKEN": "read-token",
    "ARTIFACT_ID": "7",
    "ARTIFACT_JOB": "image-base",
    "PLATFORMS": "amd64,arm64",
}


def proof_bytes(**changes):
    value = {
        "schema": 1,
        "image_id": "base",
        "image": "registry.example/base",
        "version": "1.2.3",
        "source_sha": SOURCE,
        "run_id": "42",
        "run_attempt": "2",
        "index_digest": INDEX,
        "platform_digests": PLATFORMS,
    }
    value.update(changes)
    return json.dumps(value, separators=(",", ":"), sort_keys=True).encode()


def archive(payload, name="index-proof.json"):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", zipfile.ZIP_DEFLATED) as output:
        output.writestr(name, payload)
    return stream.getvalue()


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.previous = Path.cwd()
        os.chdir(self.temporary.name)
        self.environment = patch.dict(os.environ, ENVIRONMENT, clear=False)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.proof = proof_bytes()
        self.payload = archive(self.proof)
        self.archive_digest = hashlib.sha256(self.payload).hexdigest()
        os.environ["ARTIFACT_DIGEST"] = "sha256:" + self.archive_digest
        self.run = {
            "id": 42,
            "run_attempt": 2,
            "head_sha": SOURCE,
            "repository": {"id": 9, "full_name": "example/repo"},
        }
        self.job = {
            "name": "image-base",
            "run_id": 42,
            "run_attempt": 2,
            "head_sha": SOURCE,
            "status": "completed",
            "conclusion": "success",
        }
        self.artifact = {
            "id": 7,
            "name": "oci-index-42-2-base",
            "expired": False,
            "size_in_bytes": len(self.payload),
            "digest": "sha256:" + self.archive_digest,
            "workflow_run": {
                "id": 42,
                "head_sha": SOURCE,
                "repository_id": 9,
                "head_repository_id": 9,
            },
        }

    def tearDown(self):
        os.chdir(self.previous)

    def api_documents(self, artifact=None):
        selected = artifact or self.artifact
        return [self.run, selected, self.run, selected]

    def invoke(self, documents=None, payload=None, jobs=None):
        with patch.object(receipt, "api_json", side_effect=documents or self.api_documents()), \
             patch.object(oci_digest, "api_json", return_value={"jobs": [jobs or self.job]}), \
             patch.object(receipt, "auth_request", return_value=self.payload if payload is None else payload), \
             patch.object(receipt, "output_values") as output:
            receipt.download_receipt()
        return output

    def test_exact_rest_receipt_and_proof_emit_index_digest(self):
        output = self.invoke()
        output.assert_called_once_with({"index_digest": INDEX})

    def test_explicit_artifact_id_and_zip_digest_required(self):
        for variable in ("ARTIFACT_ID", "ARTIFACT_DIGEST"):
            with self.subTest(variable=variable):
                os.environ.pop(variable)
                with self.assertRaisesRegex(receipt.GateError, "missing_" + variable.lower()):
                    receipt.context()
                os.environ[variable] = "7" if variable == "ARTIFACT_ID" else "sha256:" + self.archive_digest

    def test_artifact_name_id_digest_and_source_are_exact(self):
        mutations = [
            {"id": 8},
            {"name": "oci-42-2-other"},
            {"digest": "sha256:" + "f" * 64},
            {"workflow_run": dict(self.artifact["workflow_run"], **{"head_sha": "b" * 40})},
        ]
        for mutation in mutations:
            artifact = dict(self.artifact, **mutation)
            with self.subTest(mutation=mutation), self.assertRaises(receipt.GateError):
                self.invoke(self.api_documents(artifact))

    def test_producer_must_be_current_image_job_success(self):
        for key, value in (("name", "image-other"), ("run_attempt", 1), ("head_sha", "b" * 40),
                           ("status", "in_progress"), ("conclusion", "failure")):
            with self.subTest(key=key), self.assertRaises(receipt.GateError):
                self.invoke(self.api_documents(), jobs=dict(self.job, **{key: value}))

    def test_zip_digest_is_checked_before_proof_parse(self):
        with patch.object(receipt, "receipt_member", side_effect=AssertionError("opened early")), \
             self.assertRaisesRegex(receipt.GateError, "receipt_archive_digest"):
            self.invoke(payload=b"tampered")

    def test_canonical_proof_and_exact_platform_set_required(self):
        for raw in (
            self.proof + b"\n",
            archive(proof_bytes(platform_digests={"amd64": PLATFORMS["amd64"]})),
            archive(proof_bytes(platform_digests={**PLATFORMS, "s390x": PLATFORMS["amd64"]})),
        ):
            payload = raw if raw.startswith(b"PK") else archive(raw)
            artifact = dict(self.artifact, size_in_bytes=len(payload), digest="sha256:" + hashlib.sha256(payload).hexdigest())
            os.environ["ARTIFACT_DIGEST"] = artifact["digest"]
            with self.subTest(payload=payload[:10]), self.assertRaises(receipt.GateError):
                self.invoke(self.api_documents(artifact), payload=payload)

    def test_index_digest_is_distinct_from_zip_digest(self):
        os.environ["ARTIFACT_DIGEST"] = INDEX
        with self.assertRaises(receipt.GateError):
            self.invoke()

    def test_second_metadata_change_is_rejected(self):
        changed = dict(self.artifact, size_in_bytes=self.artifact["size_in_bytes"] + 1)
        with self.assertRaisesRegex(receipt.GateError, "receipt_artifact_changed"):
            self.invoke([self.run, self.artifact, self.run, changed])


if __name__ == "__main__":
    unittest.main()
