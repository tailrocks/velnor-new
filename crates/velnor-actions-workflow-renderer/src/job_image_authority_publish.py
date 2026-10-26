# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Fixed image publication assembly, byte verification, and evidence.

Data-only port of the publication orchestration in
``images/job/ubuntu-26.04/candidate/publish.py``. It binds the signed
record to its archive and OCI graph, extracts and verifies blob bytes from
the local archive, and builds the signed publication evidence. It never
runs candidate code, never reads process state or a checkout, and
never performs registry uploads itself: uploads stay in the rendered
authority workflow, guarded by the registry rules in
``job_image_authority_registry``. It performs no uploads itself.
"""
import hashlib
import json
import re
import tarfile

EVIDENCE_SCHEMA = "velnor.job-image-publication.v1"
PACKAGE = "ghcr.io/tailrocks/velnor-job"
MAX_MANIFEST_BYTES = 2 * 1024 * 1024
MAX_UPLOAD_BYTES = 1024**3
MAX_RESULT_BYTES = 64 * 1024


class AuthorityError(ValueError):
    pass


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_constant(value):
    raise AuthorityError(f"JSON constant has an incorrect value: {value}.")


def _json_bytes(data, label):
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"{label} has an incorrect format.") from error


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def check_record_binding(record, archive_digest, archive_size, oci):
    if not isinstance(record, dict):
        raise AuthorityError("Candidate record has an incorrect format.")
    if record.get("archive") != {"sha256": archive_digest, "size_bytes": archive_size}:
        raise AuthorityError("Candidate archive is not the same as the archive in its signed record.")
    if record.get("oci") != oci:
        raise AuthorityError("Candidate OCI graph is not the same as the graph in its signed record.")
    return record


def blob_descriptors(manifest):
    config = manifest.get("config") if isinstance(manifest, dict) else None
    layers = manifest.get("layers") if isinstance(manifest, dict) else None
    if not isinstance(config, dict) or not isinstance(layers, list):
        raise AuthorityError("OCI manifest blob descriptors are incorrect.")
    unique = {}
    for descriptor in [config, *layers]:
        if not isinstance(descriptor, dict):
            raise AuthorityError("OCI blob descriptor is incorrect.")
        digest = descriptor.get("digest")
        size = descriptor.get("size")
        if (
            not isinstance(digest, str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
            or type(size) is not int
            or not 1 <= size <= MAX_UPLOAD_BYTES
        ):
            raise AuthorityError("OCI blob descriptor is incorrect.")
        if digest in unique and unique[digest] != size:
            raise AuthorityError("Repeated OCI blob digest has inconsistent sizes.")
        unique[digest] = size
    return unique


def _archive_blob(archive, digest, limit):
    try:
        with tarfile.open(archive, "r:") as tar:
            member = tar.getmember(f"blobs/sha256/{digest}")
            if not member.isfile() or member.size < 0:
                raise AuthorityError("OCI blob descriptor size is incorrect.")
            stream = tar.extractfile(member)
            if stream is None:
                raise AuthorityError("OCI blob is missing from archive.")
            with stream:
                return stream.read(limit + 1)
    except (OSError, KeyError, tarfile.TarError) as error:
        raise AuthorityError("Cannot read OCI blob from archive.") from error


def read_manifest_bytes(archive, digest, size):
    if type(size) is not int or size > MAX_MANIFEST_BYTES:
        raise AuthorityError("OCI manifest exceeds its size limit.")
    raw = _archive_blob(archive, digest.removeprefix("sha256:"), MAX_MANIFEST_BYTES)
    if len(raw) != size or _sha(raw) != digest.removeprefix("sha256:"):
        raise AuthorityError("OCI platform manifest bytes changed after validation.")
    return raw


def extract_blob(archive, digest, expected_size, directory):
    hexdigest = digest.removeprefix("sha256:")
    if not re.fullmatch(r"[0-9a-f]{64}", hexdigest) or type(expected_size) is not int or expected_size > MAX_UPLOAD_BYTES:
        raise AuthorityError("OCI blob descriptor size is incorrect.")
    output = directory / hexdigest
    hasher = hashlib.sha256()
    size = 0
    try:
        with tarfile.open(archive, "r:") as tar:
            member = tar.getmember(f"blobs/sha256/{hexdigest}")
            if not member.isfile() or member.size != expected_size:
                raise AuthorityError("OCI blob descriptor size is incorrect.")
            source = tar.extractfile(member)
            if source is None:
                raise AuthorityError("OCI blob is missing from archive.")
            with source, output.open("xb") as destination:
                while chunk := source.read(1024 * 1024):
                    size += len(chunk)
                    if size > expected_size:
                        raise AuthorityError("OCI blob exceeds descriptor size.")
                    hasher.update(chunk)
                    destination.write(chunk)
    except (OSError, KeyError, tarfile.TarError) as error:
        raise AuthorityError("Cannot read OCI blob from archive.") from error
    if size != expected_size or hasher.hexdigest() != hexdigest:
        raise AuthorityError("OCI blob bytes are not the same as the bytes identified by their descriptor.")
    return output


def build_evidence(identity, ci, record_archive, oci, manifest, record_sha256):
    manifest_value = _json_bytes(manifest, "OCI manifest")
    if not isinstance(manifest_value, dict):
        raise AuthorityError("OCI platform manifest has an incorrect format.")
    config = manifest_value.get("config", {})
    layers = manifest_value.get("layers", [])
    evidence = {
        "schema": EVIDENCE_SCHEMA,
        "status": "PASS",
        "package": PACKAGE,
        "digest_reference": f"{PACKAGE}@{oci['platform_manifest_digest']}",
        "private_package_access_gate": "not_proven",
        "archive": {"sha256": record_archive["sha256"], "size_bytes": record_archive["size_bytes"]},
        "source": identity["source"],
        "workflow": {
            "ref": identity["workflow_ref"],
            "authority_sha": identity["source"],
            "run_id": identity["run_id"],
            "run_attempt": identity["run_attempt"],
            "artifact_id": identity["artifact_id"],
        },
        "ci": ci,
        "oci": {
            "platform": "linux/amd64",
            "archive_index_digest": "sha256:" + oci["archive_index_digest"],
            "manifest_digest": oci["platform_manifest_digest"],
            "manifest_size_bytes": len(manifest),
            "config_digest": config.get("digest"),
            "layer_digests": [layer.get("digest") for layer in layers],
        },
        "record_sha256": record_sha256,
    }
    encoded = (json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if len(encoded) > MAX_RESULT_BYTES:
        raise AuthorityError("Publication evidence exceeds its size limit.")
    return encoded


def validate_evidence(value):
    expected_keys = {
        "schema", "status", "package", "digest_reference",
        "private_package_access_gate", "archive", "source", "workflow",
        "ci", "oci", "record_sha256",
    }
    if not isinstance(value, dict) or set(value) != expected_keys:
        raise AuthorityError("Publication evidence has unknown or missing fields.")
    if value["schema"] != EVIDENCE_SCHEMA or value["status"] != "PASS":
        raise AuthorityError("Publication evidence schema or status is incorrect.")
    if value["package"] != PACKAGE:
        raise AuthorityError("Publication evidence package is incorrect.")
    oci = value.get("oci")
    if not isinstance(oci, dict) or oci.get("platform") != "linux/amd64":
        raise AuthorityError("Publication evidence OCI platform is incorrect.")
    for name in ("manifest_digest", "config_digest"):
        digest = oci.get(name)
        if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise AuthorityError(f"Publication evidence {name} is incorrect.")
    if type(oci.get("manifest_size_bytes")) is not int or oci["manifest_size_bytes"] < 1:
        raise AuthorityError("Publication evidence manifest size is incorrect.")
    if (
        not isinstance(oci.get("layer_digests"), list)
        or not oci["layer_digests"]
        or any(not isinstance(item, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", item) for item in oci["layer_digests"])
    ):
        raise AuthorityError("Publication evidence layer digests are incorrect.")
    index_digest = oci.get("archive_index_digest")
    if not isinstance(index_digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", index_digest):
        raise AuthorityError("Publication evidence archive index digest is incorrect.")
    if value["digest_reference"] != f"{PACKAGE}@{oci['manifest_digest']}":
        raise AuthorityError("Publication evidence digest reference is incorrect.")
    if not isinstance(value.get("source"), str) or not re.fullmatch(r"[0-9a-f]{40}", value["source"]):
        raise AuthorityError("Publication evidence source is incorrect.")
    archive = value.get("archive")
    if (
        not isinstance(archive, dict)
        or not isinstance(archive.get("sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", archive["sha256"])
        or type(archive.get("size_bytes")) is not int
        or archive["size_bytes"] < 1
    ):
        raise AuthorityError("Publication evidence archive is incorrect.")
    workflow = value.get("workflow")
    if not isinstance(workflow, dict) or set(workflow) != {"ref", "authority_sha", "run_id", "run_attempt", "artifact_id"}:
        raise AuthorityError("Publication evidence workflow has unknown or missing fields.")
    if (
        not isinstance(workflow.get("ref"), str)
        or not workflow["ref"].endswith("@refs/heads/main")
        or not isinstance(workflow.get("authority_sha"), str)
        or not re.fullmatch(r"[0-9a-f]{40}", workflow["authority_sha"])
    ):
        raise AuthorityError("Publication evidence workflow identity is incorrect.")
    for name in ("run_id", "run_attempt", "artifact_id"):
        if type(workflow.get(name)) is not int or workflow[name] < 1:
            raise AuthorityError(f"Publication evidence workflow {name} is incorrect.")
    ci = value.get("ci")
    if (
        not isinstance(ci, dict)
        or set(ci) != {"run_id", "run_attempt"}
        or type(ci.get("run_id")) is not int
        or type(ci.get("run_attempt")) is not int
    ):
        raise AuthorityError("Publication evidence CI identity is incorrect.")
    if not isinstance(value.get("record_sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", value["record_sha256"]):
        raise AuthorityError("Publication evidence record digest is incorrect.")
    if value.get("private_package_access_gate") != "not_proven":
        raise AuthorityError("Publication evidence access gate is incorrect.")
    return value
