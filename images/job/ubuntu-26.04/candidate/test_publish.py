#!/usr/bin/env python3
"""Test publisher identity checks and registry request boundaries."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
from urllib.parse import parse_qs, urlsplit


MODULE_PATH = Path(__file__).with_name("publish.py")
SPEC = importlib.util.spec_from_file_location("job_image_publish", MODULE_PATH)
publish = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = publish
SPEC.loader.exec_module(publish)


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def identity_env() -> dict[str, str]:
    return {
        "GITHUB_REPOSITORY": "tailrocks/velnor-new",
        "GITHUB_REPOSITORY_ID": "1390620900",
        "GITHUB_REF": "refs/heads/main",
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_WORKFLOW_REF": publish.WORKFLOW + "@refs/heads/main",
        "GITHUB_SHA": "a" * 40,
        "GITHUB_WORKFLOW_SHA": "a" * 40,
        "GITHUB_RUN_ID": "100",
        "GITHUB_RUN_ATTEMPT": "1",
        "GH_TOKEN": "secret-management-token-123456",
        "GITHUB_ACTOR": "builder",
        "VELNOR_CANDIDATE_ARTIFACT_ID": "501",
    }


class FakeTransport:
    def __init__(self, responses=()):
        self.responses = list(responses)
        self.requests = []

    def request(self, method, url, headers, body=None, length=0):
        self.requests.append((method, url, headers.copy(), body, length))
        response = self.responses.pop(0)
        if isinstance(response, Exception):
            raise response
        return response


class PublisherTests(unittest.TestCase):
    def test_wrong_ref_source_and_run_identity_fail_before_api(self):
        for name, value in (
            ("GITHUB_REF", "refs/heads/fix/job-image-bootstrap"),
            ("GITHUB_SHA", "b" * 40),
            ("GITHUB_WORKFLOW_SHA", "b" * 40),
            ("GITHUB_REPOSITORY_ID", str(publish.REPOSITORY_ID + 1)),
            ("GITHUB_RUN_ID", "0"),
            ("VELNOR_CANDIDATE_ARTIFACT_ID", "bad"),
        ):
            env = identity_env()
            env[name] = value
            with self.subTest(name=name), mock.patch.dict(publish.os.environ, env, clear=True):
                with self.assertRaises(publish.PublishError):
                    publish._current_identity(Path("."))

    def test_candidate_source_and_run_must_match_dispatch(self):
        identity = {
            "repository_id": publish.REPOSITORY_ID, "workflow_ref": publish.WORKFLOW + "@refs/heads/main",
            "source": "a" * 40, "run_id": 100, "run_attempt": 2,
        }
        record = {
            "source": {"commit": "a" * 40, "ref": "refs/heads/main"},
            "workflow": {
                "repository": publish.REPOSITORY, "repository_id": publish.REPOSITORY_ID,
                "event": "workflow_dispatch", "workflow_ref": identity["workflow_ref"],
                "authority_commit": "a" * 40, "run_id": 100, "run_attempt": 2,
            },
        }
        publish._check_candidate_identity(record, identity)
        for key, value in (("commit", "b" * 40), ("ref", "refs/heads/feature")):
            changed = json.loads(json.dumps(record))
            changed["source"][key] = value
            with self.subTest(source_key=key), self.assertRaises(publish.PublishError):
                publish._check_candidate_identity(changed, identity)
        changed = json.loads(json.dumps(record))
        changed["workflow"]["run_attempt"] = 1
        with self.assertRaisesRegex(publish.PublishError, "run identity"):
            publish._check_candidate_identity(changed, identity)

    def test_artifact_id_name_and_run_must_match(self):
        identity = {"run_id": 100, "artifact_id": 501, "source": "a" * 40}
        payload = {"artifacts": [{
            "id": 502, "name": publish.ARTIFACT, "expired": False,
            "workflow_run": {"id": 100, "head_sha": "a" * 40, "head_branch": "main"},
        }]}
        with mock.patch.object(publish, "_api", return_value=payload):
            with self.assertRaisesRegex(publish.PublishError, "artifact ID"):
                publish._check_artifact(identity)

    def test_repository_id_must_match_canonical_repository(self):
        identity = {"repository_id": publish.REPOSITORY_ID, "source": "a" * 40}
        responses = iter([
            {"id": publish.REPOSITORY_ID + 1, "full_name": publish.REPOSITORY},
        ])
        with mock.patch.object(publish, "_api", side_effect=lambda _path: next(responses)):
            with self.assertRaisesRegex(publish.PublishError, "repository ID"):
                publish._assert_current_main(identity)
        responses = iter([
            {"id": publish.REPOSITORY_ID, "full_name": publish.REPOSITORY},
            {"sha": "b" * 40},
        ])
        with mock.patch.object(publish, "_api", side_effect=lambda _path: next(responses)):
            with self.assertRaisesRegex(publish.PublishError, "current main"):
                publish._assert_current_main(identity)

    def test_failed_or_noncanonical_ci_run_is_rejected(self):
        identity = "a" * 40
        run = {
            "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
            "head_sha": identity, "head_repository": {"full_name": publish.REPOSITORY},
            "run_number": 3, "run_attempt": 1, "id": 7,
            "status": "completed", "conclusion": "failure",
        }
        with mock.patch.object(publish, "_api", return_value={"workflow_runs": [run]}):
            with self.assertRaisesRegex(publish.PublishError, "did not succeed"):
                publish._check_ci(identity)
        run["conclusion"] = "success"
        run["path"] = ".github/workflows/other.yml"
        with mock.patch.object(publish, "_api", return_value={"workflow_runs": [run]}):
            with self.assertRaisesRegex(publish.PublishError, "No canonical"):
                publish._check_ci(identity)

    def test_attestation_failure_is_generic_and_does_not_print_token(self):
        secret = identity_env()["GH_TOKEN"]
        with mock.patch.object(publish, "_gh", side_effect=publish.PublishError("verification failed")):
            with self.assertRaises(publish.PublishError) as error:
                publish._verify_attestation(Path("candidate.tar"), "a" * 40)
        self.assertNotIn(secret, str(error.exception))

    def test_attestation_json_binds_repository_workflow_ref_and_source(self):
        source = "a" * 40
        certificate = {
            "sourceRepositoryIdentifier": "1390620900",
            "githubWorkflowRepository": publish.REPOSITORY,
            "githubWorkflowSHA": source,
            "githubWorkflowRef": "refs/heads/main",
            "githubWorkflowTrigger": "workflow_dispatch",
            "sourceRepositoryRef": "refs/heads/main",
            "sourceRepositoryDigest": source,
        }
        valid = [{"verificationResult": {"signature": {"certificate": certificate}}}]
        with mock.patch.object(publish, "_gh", return_value=json.dumps(valid).encode()) as gh:
            publish._verify_attestation(Path("candidate.tar"), source)
            self.assertIn("--format", gh.call_args.args[0])
        invalid_variants = (
            {**certificate, "sourceRepositoryIdentifier": "1390620901"},
            {**certificate, "githubWorkflowRepository": "attacker/velnor-new"},
            {**certificate, "githubWorkflowSHA": "b" * 40},
            {**certificate, "githubWorkflowRef": "refs/heads/feature"},
            {**certificate, "githubWorkflowTrigger": "push"},
            {**certificate, "sourceRepositoryRef": "refs/heads/feature"},
            {**certificate, "sourceRepositoryDigest": "b" * 40},
        )
        for bad_certificate in invalid_variants:
            bad = [{"verificationResult": {"signature": {"certificate": bad_certificate}}}]
            with self.subTest(certificate=bad_certificate), mock.patch.object(
                publish, "_gh", return_value=json.dumps(bad).encode()
            ), self.assertRaises(publish.PublishError):
                publish._verify_attestation(Path("candidate.tar"), source)

    def test_repeated_layer_digest_is_uploaded_once(self):
        blob = b"layer bytes"
        descriptor = {"digest": digest(blob), "size": len(blob)}
        manifest = {"config": descriptor, "layers": [descriptor, descriptor]}
        self.assertEqual(publish._blob_descriptors(manifest), {digest(blob): len(blob)})
        with self.assertRaisesRegex(publish.PublishError, "inconsistent sizes"):
            publish._blob_descriptors({"config": descriptor,
                                       "layers": [{"digest": digest(blob), "size": len(blob) + 1}]})

    def test_registry_auth_challenge_and_token_are_validated(self):
        token = "registry-token-value-1234567890"
        transport = FakeTransport([
            publish._Response(401, {
                "WWW-Authenticate": 'Bearer realm="https://evil.test/token",service="ghcr.io",scope="repository:user/image:pull"',
            }, b""),
        ])
        with self.assertRaisesRegex(publish.PublishError, "challenge"):
            publish._auth_token(transport, "actor", "hidden-token")
        good = FakeTransport([
            publish._Response(401, {
                "WWW-Authenticate": 'Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:user/image:pull"',
            }, b""),
            publish._Response(200, {}, json.dumps({"token": token}).encode()),
        ])
        self.assertEqual(publish._auth_token(good, "actor", "hidden-token"), token)
        self.assertEqual(good.requests[0][0], "GET")
        token_query = parse_qs(urlsplit(good.requests[1][1]).query)
        self.assertEqual(token_query, {
            "service": ["ghcr.io"],
            "scope": ["repository:tailrocks/velnor-job:pull,push"],
        })
        self.assertNotIn("hidden-token", repr(good.responses))

    def test_auth_challenge_does_not_accept_unknown_service_or_fields(self):
        for challenge in (
            'Bearer realm="https://ghcr.io/token",service="evil.test"',
            'Bearer realm="https://ghcr.io/token",service="ghcr.io",ignored="x"',
        ):
            transport = FakeTransport([publish._Response(401, {"WWW-Authenticate": challenge}, b"")])
            with self.subTest(challenge=challenge), self.assertRaises(publish.PublishError):
                publish._auth_token(transport, "actor", "hidden-token")

    def test_redirect_and_untrusted_upload_location_are_rejected(self):
        redirect = FakeTransport([publish._Response(302, {"Location": "https://evil.test/"}, b"")])
        with self.assertRaisesRegex(publish.PublishError, "redirects"):
            publish._request(redirect, "secret-token-value-123456", "GET", "https://ghcr.io/v2/", {})
        for location in ("https://evil.test/v2/tailrocks/velnor-job/blobs/uploads/x",
                         "http://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/x",
                         "https://ghcr.io/v2/tailrocks/other/blobs/uploads/x"):
            with self.subTest(location=location), self.assertRaises(publish.PublishError):
                publish._validate_location("https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/", location)

    def test_registry_digest_and_manifest_byte_mismatch_fail(self):
        content = b"exact manifest"
        wrong = FakeTransport([publish._Response(200, {}, b"tampered value")])
        with self.assertRaisesRegex(publish.PublishError, "wrong digest"):
            publish._verify_get(wrong, "token-value-1234567890", "manifests", digest(content), content, len(content))
        bad_head = FakeTransport([publish._Response(200, {
            "Docker-Content-Digest": digest(b"different"), "Content-Length": str(len(content)),
        }, b"")])
        with self.assertRaisesRegex(publish.PublishError, "HEAD"):
            publish._head_object(bad_head, "token-value-1234567890", "blobs", digest(content), len(content))

    def test_manifest_reopen_checks_descriptor_size_and_digest(self):
        original = b"verified platform manifest"
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "candidate.tar"
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode="w:") as tar:
                info = tarfile.TarInfo("blobs/sha256/" + hashlib.sha256(original).hexdigest())
                info.size = len(original)
                tar.addfile(info, io.BytesIO(original))
            archive.write_bytes(buffer.getvalue())
            self.assertEqual(publish._read_manifest(archive, digest(original), len(original)), original)
            changed = b"tampered platform manifest"
            self.assertEqual(len(changed), len(original))
            tar_bytes = io.BytesIO()
            with tarfile.open(fileobj=tar_bytes, mode="w:") as tar:
                info = tarfile.TarInfo("blobs/sha256/" + hashlib.sha256(original).hexdigest())
                info.size = len(changed)
                tar.addfile(info, io.BytesIO(changed))
            archive.write_bytes(tar_bytes.getvalue())
            with self.assertRaisesRegex(publish.PublishError, "changed after validation"):
                publish._read_manifest(archive, digest(original), len(original))
            with self.assertRaisesRegex(publish.PublishError, "changed after validation"):
                publish._read_manifest(archive, digest(original), len(original) + 1)

    def test_partial_upload_after_timeout_requires_digest_head(self):
        blob = b"upload bytes"
        blob_digest = digest(blob)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "blob"
            path.write_bytes(blob)
            missing = FakeTransport([
                publish._Response(404, {}, b""),
                publish._Response(202, {"Location": "/v2/tailrocks/velnor-job/blobs/uploads/session"}, b""),
                publish.PublishError("request failed"),
                publish._Response(404, {}, b""),
            ])
            with self.assertRaises(publish.PublishError):
                publish._upload_blob(missing, "secret-token-value-123456", blob_digest, path)
            already_uploaded = FakeTransport([
                publish._Response(404, {}, b""),
                publish._Response(202, {"Location": "/v2/tailrocks/velnor-job/blobs/uploads/session"}, b""),
                publish.PublishError("request failed"),
                publish._Response(200, {
                    "Docker-Content-Digest": blob_digest, "Content-Length": str(len(blob)),
                }, b""),
            ])
            publish._upload_blob(already_uploaded, "secret-token-value-123456", blob_digest, path)

    def test_token_never_appears_in_public_error(self):
        secret = "management-token-secret-0123456789"
        transport = FakeTransport([publish._Response(401, {}, b"server echoed " + secret.encode())])
        with self.assertRaises(publish.PublishError) as error:
            publish._auth_token(transport, "actor", secret)
        self.assertNotIn(secret, str(error.exception))

    def test_https_timeout_is_bounded_and_redacted(self):
        class TimedOutConnection:
            def __init__(self, host, timeout):
                self.host = host
                self.timeout = timeout

            def putrequest(self, method, path):
                pass

            def putheader(self, name, value):
                pass

            def endheaders(self):
                raise TimeoutError("server echoed secret-token")

            def close(self):
                pass

        secret = "secret-token-value-123456"
        with mock.patch.object(publish.http.client, "HTTPSConnection", TimedOutConnection):
            with self.assertRaisesRegex(publish.PublishError, "timed out") as error:
                publish._HttpsTransport().request(
                    "GET", "https://ghcr.io/v2/", {"Authorization": f"Bearer {secret}"}
                )
        self.assertNotIn(secret, str(error.exception))

    def test_uncertain_manifest_upload_is_checked_by_digest_and_bytes(self):
        manifest = b"oci manifest bytes"
        manifest_digest = digest(manifest)
        transport = FakeTransport([
            publish._Response(404, {}, b""),
            publish.PublishError("timed out"),
            publish._Response(200, {
                "Docker-Content-Digest": manifest_digest,
                "Content-Length": str(len(manifest)),
            }, b""),
            publish._Response(200, {"Docker-Content-Digest": manifest_digest}, manifest),
        ])
        publish._upload_manifest(transport, "secret-token-value-123456", manifest, manifest_digest)


if __name__ == "__main__":
    unittest.main()
