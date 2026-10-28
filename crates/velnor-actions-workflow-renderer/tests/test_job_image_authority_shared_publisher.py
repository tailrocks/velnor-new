# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_registry as registry_module
from job_image_authority_shared_publisher_fixture import RegistryIO, make_oci_archive


SOURCE_SHA = "a" * 40
PACKAGES = (
    "tailrocks/velnor-job",
    "tailrocks/velnor-runner",
    "tailrocks/velnor-dind",
    "tailrocks/velnor-resource-probe",
)
EVIDENCE_KEYS = {
    "schema", "status", "package", "digest_reference",
    "private_package_access_gate", "archive", "source", "workflow",
    "ci", "oci", "record_sha256",
}
PUBLICATION_ERRORS = (publish_module.AuthorityError, registry_module.AuthorityError)


def prepare(root, package, source_sha=SOURCE_SHA, transport_package=None, tamper=None):
    archive = Path(root) / "image.tar"
    make_oci_archive(archive)
    oci = oci_module.verify_oci_archive(archive)
    manifest = publish_module.read_manifest_bytes(
        archive, oci["platform_manifest_digest"], oci["platform_manifest_size_bytes"]
    )
    registry = registry_module.package_scope(package, source_sha)
    transport = RegistryIO(transport_package or package, tamper_readback=tamper)
    token = publish_module.fetch_registry_token(
        registry, transport, "fixture-actor", "fixture-password"
    )
    blob_directory = Path(root) / "blobs"
    blob_directory.mkdir()
    manifest_value = json.loads(manifest)
    blob_paths = {
        digest: publish_module.extract_blob(
            archive, digest, size, blob_directory
        )
        for digest, size in publish_module.blob_descriptors(manifest_value).items()
    }
    return archive, oci, manifest, registry, transport, token, blob_paths


def evidence_for(oci, manifest, proof):
    identity = {
        "source": SOURCE_SHA,
        "workflow_ref": "tailrocks/velnor/.github/workflows/release.yml@refs/heads/main",
        "run_id": 71,
        "run_attempt": 1,
        "artifact_id": 93,
    }
    evidence = publish_module.build_evidence(
        identity,
        {"run_id": 72, "run_attempt": 1},
        {"sha256": "b" * 64, "size_bytes": 2048},
        oci,
        manifest,
        "c" * 64,
        {"commit": SOURCE_SHA, "ref": "refs/heads/main"},
        proof,
    )
    return publish_module.validate_evidence(json.loads(evidence))


class SharedPublisherTests(unittest.TestCase):
    def publish(self, root, package, source_sha=SOURCE_SHA, transport_package=None, tamper=None):
        state = prepare(root, package, source_sha, transport_package, tamper)
        proof = publish_module.publish_image(
            state[3], state[4], state[5], state[2], state[6]
        )
        return (*state, proof)

    def test_all_admitted_packages_publish_verified_bytes_and_source_tag(self):
        for package in PACKAGES:
            with self.subTest(package=package), tempfile.TemporaryDirectory() as root:
                _archive, _oci, manifest, scope, io_double, _token, paths, proof = self.publish(root, package)
                digest = "sha256:" + hashlib.sha256(manifest).hexdigest()
                self.assertEqual(io_double.token_scope, f"repository:{package}:pull,push")
                self.assertEqual(io_double.manifests[digest], manifest)
                self.assertEqual(io_double.tags[f"sha-{SOURCE_SHA}"], digest)
                self.assertEqual(io_double.manifests[io_double.tags[f"sha-{SOURCE_SHA}"]], manifest)
                for blob_digest, path in paths.items():
                    self.assertEqual(io_double.blobs[blob_digest], path.read_bytes())
                self.assertEqual(set(proof), {"blobs", "manifest"})
                self.assertEqual(scope.PACKAGE, package)

    def test_job_evidence_shape_and_legacy_default_are_preserved(self):
        with tempfile.TemporaryDirectory() as root:
            _archive, oci, manifest, scope, _io_double, _token, paths, proof = self.publish(
                root, "tailrocks/velnor-job"
            )
            evidence = evidence_for(oci, manifest, proof)
            self.assertEqual(set(evidence), EVIDENCE_KEYS)
            self.assertEqual(evidence["package"], publish_module.PACKAGE)
            self.assertEqual(
                evidence["digest_reference"],
                f"{publish_module.PACKAGE}@{oci['platform_manifest_digest']}",
            )
            self.assertEqual(evidence["source"], SOURCE_SHA)
            self.assertNotIn("source_tag", evidence)
            legacy_io = RegistryIO("tailrocks/velnor-job")
            legacy_token = publish_module.fetch_registry_token(
                registry_module, legacy_io, "fixture-actor", "fixture-password"
            )
            legacy_proof = publish_module.publish_image(
                registry_module, legacy_io, legacy_token, manifest, paths
            )
            legacy_evidence = evidence_for(oci, manifest, legacy_proof)
            self.assertEqual(set(legacy_evidence), EVIDENCE_KEYS)
            self.assertNotIn("sha-" + SOURCE_SHA, legacy_io.tags)
            self.assertEqual(scope.PACKAGE, "tailrocks/velnor-job")

    def test_same_source_tag_and_digest_is_idempotent(self):
        with tempfile.TemporaryDirectory() as root:
            state = self.publish(root, "tailrocks/velnor-runner")
            tag = "sha-" + SOURCE_SHA
            proof = publish_module.publish_image(state[3], state[4], state[5], state[2], state[6])
            self.assertEqual(state[4].tags[tag], proof["manifest"]["digest"])
            self.assertEqual(state[4].manifests[state[4].tags[tag]], state[2])
            self.assertEqual(set(proof), {"blobs", "manifest"})

    def test_different_manifest_cannot_replace_existing_source_tag(self):
        with tempfile.TemporaryDirectory() as root:
            state = prepare(root, "tailrocks/velnor-dind")
            tag = "sha-" + SOURCE_SHA
            old_manifest = json.dumps(json.loads(state[2]), sort_keys=True, indent=1).encode()
            self.assertNotEqual(old_manifest, state[2])
            state[4].seed_tag(tag, old_manifest)
            old_digest = state[4].tags[tag]
            proof = None
            with self.assertRaises(PUBLICATION_ERRORS):
                proof = publish_module.publish_image(state[3], state[4], state[5], state[2], state[6])
            self.assertIsNone(proof)
            self.assertEqual(state[4].tags[tag], old_digest)
            self.assertEqual(state[4].manifests[old_digest], old_manifest)

    def test_scope_is_closed_immutable_and_bound_to_registry_paths(self):
        with self.assertRaises(registry_module.AuthorityError):
            registry_module.package_scope("tailrocks/velnor-other", SOURCE_SHA)
        with self.assertRaises(registry_module.AuthorityError):
            registry_module.registry_url("manifests/latest", "tailrocks/velnor-other")
        scope = registry_module.package_scope("tailrocks/velnor-runner", SOURCE_SHA)
        with self.assertRaises(AttributeError):
            scope._package = "tailrocks/velnor-dind"
        with tempfile.TemporaryDirectory() as root:
            state = prepare(root, "tailrocks/velnor-runner")
            scope = state[3]
            try:
                del scope._source_sha
            except AttributeError:
                pass
            proof = publish_module.publish_image(scope, state[4], state[5], state[2], state[6])
            self.assertEqual(
                state[4].tags.get(f"sha-{SOURCE_SHA}"), proof["manifest"]["digest"]
            )
        with tempfile.TemporaryDirectory() as root:
            state = prepare(root, "tailrocks/velnor-runner", transport_package="tailrocks/velnor-dind")
            with self.assertRaises(PUBLICATION_ERRORS):
                publish_module.publish_image(state[3], state[4], state[5], state[2], state[6])
            self.assertEqual(state[4].tags, {})

    def test_changed_blob_bytes_fail_without_source_tag_or_proof(self):
        with tempfile.TemporaryDirectory() as root:
            state = prepare(root, "tailrocks/velnor-resource-probe")
            blob_path = next(iter(state[6].values()))
            blob_path.write_bytes(bytes([blob_path.read_bytes()[0] ^ 1]) + blob_path.read_bytes()[1:])
            proof = None
            with self.assertRaises(PUBLICATION_ERRORS):
                proof = publish_module.publish_image(state[3], state[4], state[5], state[2], state[6])
            self.assertIsNone(proof)
            self.assertEqual(state[4].tags, {})

    def test_failed_blob_readback_fails_without_source_tag_or_proof(self):
        with tempfile.TemporaryDirectory() as root:
            state = prepare(root, "tailrocks/velnor-runner", tamper="blob")
            proof = None
            with self.assertRaises(publish_module.AuthorityError):
                proof = publish_module.publish_image(state[3], state[4], state[5], state[2], state[6])
            self.assertIsNone(proof)
            self.assertEqual(state[4].tags, {})

    def test_failed_source_tag_readback_fails_without_publication_proof(self):
        with tempfile.TemporaryDirectory() as root:
            state = prepare(root, "tailrocks/velnor-dind", tamper="source-tag")
            proof = None
            with self.assertRaises(PUBLICATION_ERRORS):
                proof = publish_module.publish_image(state[3], state[4], state[5], state[2], state[6])
            self.assertIsNone(proof)
            self.assertEqual(state[4].tags[f"sha-{SOURCE_SHA}"], "sha256:" + hashlib.sha256(state[2]).hexdigest())


if __name__ == "__main__":
    unittest.main()
