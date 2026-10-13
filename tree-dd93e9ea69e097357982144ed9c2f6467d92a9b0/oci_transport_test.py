"""Strict immutable OCI artifact transport tests."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import sys
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
transport = load("oci_digest_parts")


SOURCE = "a" * 40
CHILD = "sha256:" + "b" * 64
ENVIRONMENT = {
    "REPOSITORY": "example/repo",
    "GITHUB_REPOSITORY": "example/repo",
    "IMAGE": "registry.example/base",
    "IMAGE_ID": "base",
    "ARCH": "amd64",
    "VERSION": "1.2.3",
    "SOURCE_SHA": SOURCE,
    "GITHUB_RUN_ID": "42",
    "GITHUB_RUN_ATTEMPT": "2",
    "GH_TOKEN": "read-token",
    "ARTIFACT_ID": "7",
    "ARTIFACT_JOB": "platform-base-amd64",
}


def record_bytes():
    value = {
        "schema": 1,
        "image_id": "base",
        "image": "registry.example/base",
        "arch": "amd64",
        "version": "1.2.3",
        "source_sha": SOURCE,
        "digest": CHILD,
        "run_id": "42",
        "run_attempt": "2",
    }
    return json.dumps(value, separators=(",", ":"), sort_keys=True).encode()


def archive(payload, name="base-amd64.json"):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", zipfile.ZIP_DEFLATED) as output:
        output.writestr(name, payload)
    return stream.getvalue()


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.previous = Path.cwd()
        os.chdir(self.temporary.name)
        self.environment = patch.dict(os.environ, ENVIRONMENT, clear=False)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.payload = archive(record_bytes())
        self.digest = hashlib.sha256(self.payload).hexdigest()
        self.run = {
            "id": 42,
            "run_attempt": 2,
            "head_sha": SOURCE,
            "repository": {"id": 9, "full_name": "example/repo"},
        }
        self.artifact = {
            "id": 7,
            "name": "oci-42-2-base-amd64",
            "expired": False,
            "size_in_bytes": len(self.payload),
            "digest": "sha256:" + self.digest,
            "workflow_run": {
                "id": 42,
                "run_attempt": 2,
                "head_sha": SOURCE,
                "repository_id": 9,
                "head_repository_id": 9,
            },
        }
        self.job = {
            "name": "platform-base-amd64",
            "run_id": 42,
            "run_attempt": 2,
            "head_sha": SOURCE,
            "status": "completed",
            "conclusion": "success",
        }
        os.environ["ARTIFACT_DIGEST"] = self.artifact["digest"]

    def tearDown(self):
        os.chdir(self.previous)

    def documents(self, artifact=None, run=None):
        return [run or self.run, {"jobs": [self.job]}, artifact or self.artifact,
                run or self.run, artifact or self.artifact]

    def test_rest_metadata_and_archive_digest_bind_record(self):
        with patch.object(transport, "api_json", side_effect=[self.run, self.artifact, self.run, self.artifact]), \
             patch.object(oci_digest, "api_json", return_value={"jobs": [self.job]}), \
             patch.object(transport, "auth_request", return_value=self.payload):
            transport.download_record()
        self.assertEqual(Path("digests/base-amd64.json").read_bytes(), record_bytes())

    def test_official_artifact_metadata_without_attempt_uses_run_attempt_api(self):
        artifact = dict(self.artifact)
        artifact["workflow_run"] = {key: value for key, value in self.artifact["workflow_run"].items() if key != "run_attempt"}
        with patch.object(transport, "api_json", side_effect=[self.run, artifact, self.run, artifact]), \
             patch.object(oci_digest, "api_json", return_value={"jobs": [self.job]}), \
             patch.object(transport, "auth_request", return_value=self.payload):
            transport.download_record()
        self.assertEqual(Path("digests/base-amd64.json").read_bytes(), record_bytes())

    def test_archive_hash_rejects_before_zip_or_write(self):
        with patch.object(transport, "api_json", side_effect=[self.run, self.artifact, self.run, self.artifact]), \
             patch.object(oci_digest, "api_json", return_value={"jobs": [self.job]}), \
             patch.object(transport, "auth_request", return_value=b"tampered"), \
             patch.object(transport, "record_member", side_effect=AssertionError("opened early")):
            with self.assertRaisesRegex(transport.GateError, "artifact_archive_digest"):
                transport.download_record()
        self.assertFalse(Path("digests").exists())

    def test_metadata_change_after_download_is_rejected(self):
        changed = dict(self.artifact, size_in_bytes=self.artifact["size_in_bytes"] + 1)
        with patch.object(transport, "api_json", side_effect=[self.run, self.artifact, self.run, changed]), \
             patch.object(oci_digest, "api_json", return_value={"jobs": [self.job]}), \
             patch.object(transport, "auth_request", return_value=self.payload):
            with self.assertRaisesRegex(transport.GateError, "artifact_metadata_changed"):
                transport.download_record()
        self.assertFalse(Path("digests").exists())

    def test_run_attempt_source_and_server_id_are_exact(self):
        for mutation in (
            {"run_attempt": 1},
            {"head_sha": "c" * 40},
            {"id": 8},
        ):
            artifact = dict(self.artifact)
            artifact["workflow_run"] = dict(self.artifact["workflow_run"], **mutation)
            if "id" in mutation:
                artifact["id"] = mutation["id"]
            with self.subTest(mutation=mutation), \
                 patch.object(transport, "api_json", side_effect=[self.run, artifact, self.run, artifact]), \
                 patch.object(oci_digest, "api_json", return_value={"jobs": [self.job]}), \
                 patch.object(transport, "auth_request", return_value=self.payload):
                with self.assertRaises(transport.GateError):
                    transport.download_record()
            self.assertFalse(Path("digests").exists())

    def test_producer_job_must_be_current_successful_attempt(self):
        for key, value in (("name", "other"), ("run_attempt", 1), ("head_sha", "c" * 40),
                           ("status", "in_progress"), ("conclusion", "failure")):
            with self.subTest(key=key), \
                 patch.object(transport, "api_json", side_effect=[self.run, self.artifact]), \
                 patch.object(oci_digest, "api_json", return_value={"jobs": [dict(self.job, **{key: value})]}):
                with self.assertRaises(transport.GateError):
                    transport.download_record()
            self.assertFalse(Path("digests").exists())

    def test_record_bytes_and_zip_members_are_exact(self):
        for payload, message in (
            (archive(record_bytes() + b"\n"), "artifact_record_bytes"),
            (archive(record_bytes(), "../base-amd64.json"), "artifact_zip_path"),
        ):
            artifact = dict(self.artifact, size_in_bytes=len(payload), digest="sha256:" + hashlib.sha256(payload).hexdigest())
            os.environ["ARTIFACT_DIGEST"] = artifact["digest"]
            with self.subTest(message=message), \
                 patch.object(transport, "api_json", side_effect=[self.run, artifact, self.run, artifact]), \
                 patch.object(oci_digest, "api_json", return_value={"jobs": [self.job]}), \
                 patch.object(transport, "auth_request", return_value=payload):
                with self.assertRaisesRegex(transport.GateError, message):
                    transport.download_record()
            self.assertFalse(Path("digests").exists())

    def test_immutable_producer_id_and_digest_are_mandatory(self):
        for key in ("ARTIFACT_ID", "ARTIFACT_DIGEST"):
            with self.subTest(key=key), patch.dict(os.environ, {key: ""}), \
                 patch.object(transport, "api_json", side_effect=AssertionError("looked up before producer binding")):
                with self.assertRaises(transport.GateError):
                    transport.download_record()

    def test_schema_boolean_is_not_integer_version(self):
        value = json.loads(record_bytes())
        value["schema"] = True
        raw = json.dumps(value, separators=(",", ":"), sort_keys=True).encode()
        with self.assertRaisesRegex(transport.GateError, "artifact_record_identity"):
            transport.validate_record_bytes(raw, transport.artifact_context())
        Path("digests").mkdir()
        Path("digests/base-amd64.json").write_bytes(raw)
        with self.assertRaisesRegex(transport.GateError, "record_binding"):
            transport.read_records("chainargos/base", "base", "1.2.3", SOURCE, ["amd64"])


if __name__ == "__main__":
    unittest.main()
