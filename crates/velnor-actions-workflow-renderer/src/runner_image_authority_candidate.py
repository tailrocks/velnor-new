# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import base64
import hashlib

import job_image_authority_oci as oci
import runner_image_authority_identity as identity

PROFILES = {
    "runner": ("velnor-runner", "images/runner/ubuntu-26.04"),
    "dind": ("velnor-dind", "images/dind"),
    "resource_probe": ("velnor-resource-probe", "images/resource-probe"),
}
RECORD_KEYS = {"schema", "image", "source", "workflow", "platform", "archive", "oci", "provenance", "lock"}
LOCK_KEYS = {"sha256", "runner_version", "docker_version", "buildx_version", "compose_version", "node_version"}
IMAGE_KEYS = {"record_sha256", "provenance", "archive", "oci"}
ENVELOPE_KEYS = {
    "schema", "original_archive_sha256", "original_archive_size_bytes", "original_index_b64",
    "buildkit_index_b64", "attestation_descriptor", "attestation_manifest_b64", "attestation_config_b64", "statement_b64",
}


class AuthorityError(ValueError):
    pass


def _closed(value, keys, label):
    if not isinstance(value, dict) or set(value) != keys:
        raise AuthorityError(f"{label} has unknown or missing fields.")
    return value


def _archive(path):
    if path.is_symlink() or not path.is_file():
        raise AuthorityError("Candidate archive must be a regular file.")
    digest = hashlib.sha256()
    count = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024**2):
            count += len(chunk)
            if count > oci.MAX_ARCHIVE_BYTES:
                raise AuthorityError("Candidate archive exceeds its size limit.")
            digest.update(chunk)
    return {"sha256": digest.hexdigest(), "size_bytes": count}


def _verify_candidate(path, raw, envelope_raw, receipt, role, source, producer_id, producer_attempt, lock_raw):
    if role not in PROFILES:
        raise AuthorityError("Runner image role is not fixed.")
    signed = _closed(receipt["images"][role], IMAGE_KEYS, "Qualified image")
    if signed["record_sha256"] != hashlib.sha256(raw).hexdigest():
        raise AuthorityError("Candidate record differs from qualified bytes.")
    record = _closed(identity.parse_document(raw, 1024**2), RECORD_KEYS, "Candidate record")
    image, context = PROFILES[role]
    expected_workflow = {
        "repository": identity.REPOSITORY, "repository_id": int(identity.REPOSITORY_ID),
        "event": "push", "workflow_ref": f"{identity.REPOSITORY}/.github/workflows/ci.yml@refs/heads/main",
        "authority_commit": source, "run_id": producer_id, "run_attempt": producer_attempt,
    }
    if (
        record["schema"] != "velnor.runner-image-publication.v2"
        or record["image"] != role
        or record["platform"] != "linux/amd64"
        or record["workflow"] != expected_workflow
    ):
        raise AuthorityError("Candidate role or producer identity differs from authority.")
    candidate_source = _closed(record["source"], {"commit", "ref", "inputs"}, "Candidate source")
    if candidate_source["commit"] != source or candidate_source["ref"] != "refs/heads/main":
        raise AuthorityError("Candidate source differs from authority.")
    lock = _closed(record["lock"], LOCK_KEYS, "Candidate lock")
    if (
        lock["sha256"] != receipt["runner_lock_sha256"]
        or lock["sha256"] != hashlib.sha256(lock_raw).hexdigest()
        or not all(isinstance(lock[key], str) and lock[key] for key in LOCK_KEYS)
    ):
        raise AuthorityError("Candidate lock differs from authenticated source bytes.")
    for key in ["archive", "oci", "provenance"]:
        if record[key] != signed[key]:
            raise AuthorityError("Candidate evidence differs from qualified evidence.")
    measured_archive = _archive(path)
    if measured_archive != record["archive"]:
        raise AuthorityError("Candidate archive differs from qualified bytes.")
    graph = oci.verify_oci_archive(path)
    if graph != record["oci"]:
        raise AuthorityError("Candidate OCI graph differs from qualified graph.")
    provenance = _closed(record["provenance"], {"envelope", "original_archive", "original_index", "named_contexts"}, "Candidate provenance")
    if provenance["envelope"] != {"sha256": hashlib.sha256(envelope_raw).hexdigest(), "size_bytes": len(envelope_raw)}:
        raise AuthorityError("Candidate provenance envelope differs from qualified bytes.")
    envelope = _closed(identity.parse_document(envelope_raw, 8 * 1024**2), ENVELOPE_KEYS, "Provenance envelope")
    if envelope["schema"] != "velnor.runner-provenance-envelope.v1":
        raise AuthorityError("Provenance envelope schema is invalid.")
    try:
        statement = base64.b64decode(envelope["statement_b64"], validate=True)
    except (ValueError, TypeError) as error:
        raise AuthorityError("Provenance statement encoding is invalid.") from error
    parsed = identity.parse_document(statement, 1024**2)
    if parsed.get("subject") != [{"name": f"pkg:docker/{image}@linux-amd64?platform=linux%2Famd64", "digest": {"sha256": graph["platform_manifest_digest"].removeprefix("sha256:")}}]:
        raise AuthorityError("Provenance statement subject differs from candidate bytes.")
    build_source = {
        "metadata_sha256": hashlib.sha256(statement).hexdigest(),
        "build_type": "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
        "source_repository": identity.REPOSITORY_URI, "source_commit": source,
        "context_path": context, "dockerfile_path": f"{context}/Dockerfile", "builder_platform": "linux/amd64",
        "named_contexts": {name: value["sha256"] for name, value in provenance["named_contexts"].items()},
    }
    if _archive(path) != measured_archive:
        raise AuthorityError("Candidate archive changed during admission.")
    return {"record": record, "graph": graph, "build_source": build_source}


def verify_candidate(path, raw, envelope_raw, receipt, role, source, producer_id, producer_attempt, lock_raw):
    try:
        return _verify_candidate(path, raw, envelope_raw, receipt, role, source, producer_id, producer_attempt, lock_raw)
    except (KeyError, TypeError, AttributeError, OSError, identity.AuthorityError, oci.AuthorityError) as error:
        raise AuthorityError("Candidate qualification evidence is invalid.") from error
