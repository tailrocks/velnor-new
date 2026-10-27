# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import json
import re

SCHEMA = "velnor.job-image-candidate.v1"
MAX_RECORD_BYTES = 1024**2
INPUT_PATHS = (
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
)
RECORD_KEYS = {
    "schema", "source", "workflow", "platform", "build", "archive", "oci"
}
SOURCE_KEYS = {"commit", "ref", "inputs"}
WORKFLOW_KEYS = {
    "repository", "repository_id", "event", "workflow_ref", "authority_commit",
    "run_id", "run_attempt",
}
BUILD_KEYS = {
    "builder", "platform", "arguments", "exporter", "provenance",
    "compression", "force_compression",
}
ARCHIVE_KEYS = {"sha256", "size_bytes"}
OCI_KEYS = {
    "archive_index_digest", "archive_index_size_bytes", "platform_manifest_digest",
    "platform_manifest_size_bytes", "image_configuration_digest",
    "image_configuration_size_bytes", "layer_digests",
}


class AuthorityError(ValueError):
    pass


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _json_bytes(data, label):
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"{label} JSON has an incorrect format.") from error


def _reject_constant(value):
    raise AuthorityError(f"JSON constant has an incorrect value: {value}.")


def _exact_keys(value, expected, label):
    if not isinstance(value, dict) or set(value) != expected:
        raise AuthorityError(f"{label} has unknown or missing fields.")
    return value


def _digest(value, label):
    if not isinstance(value, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", value):
        raise AuthorityError(f"{label} must be a lowercase SHA-256 digest.")
    return value


def _read_limited_file(path, limit, label):
    if path.is_symlink() or not path.is_file():
        raise AuthorityError(f"{label} must be a regular file.")
    if path.stat().st_size > limit:
        raise AuthorityError(f"{label} exceeds its size limit.")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise AuthorityError(f"{label} exceeds its size limit.")
    return data


def load_record(path):
    return _json_bytes(_read_limited_file(path, MAX_RECORD_BYTES, "Candidate record"), "candidate record")


def validate_record(value):
    record = _exact_keys(value, RECORD_KEYS, "Candidate record")
    if record["schema"] != SCHEMA or record["platform"] != "linux/amd64":
        raise AuthorityError("Candidate schema or platform is incorrect.")
    source = _exact_keys(record["source"], SOURCE_KEYS, "Source")
    if not isinstance(source["commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", source["commit"]) or not isinstance(source["ref"], str) or not source["ref"].startswith("refs/"):
        raise AuthorityError("Source identity is incorrect.")
    if not isinstance(source["inputs"], dict) or set(source["inputs"]) != set(INPUT_PATHS):
        raise AuthorityError("The source input list is incomplete or unknown.")
    for name, digest in source["inputs"].items():
        if not isinstance(name, str) or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise AuthorityError("Source input hash is incorrect.")
    workflow = _exact_keys(record["workflow"], WORKFLOW_KEYS, "Workflow")
    if not isinstance(workflow["repository"], str) or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", workflow["repository"]):
        raise AuthorityError("Workflow repository identity is incorrect.")
    if type(workflow["repository_id"]) is not int or workflow["repository_id"] < 1:
        raise AuthorityError("Workflow repository ID is incorrect.")
    if not isinstance(workflow["event"], str) or not re.fullmatch(r"[a-z_]+", workflow["event"]):
        raise AuthorityError("Workflow event is incorrect.")
    if (
        not isinstance(workflow["workflow_ref"], str)
        or not workflow["workflow_ref"].startswith(f"{workflow['repository']}/.github/workflows/")
        or "@refs/" not in workflow["workflow_ref"]
    ):
        raise AuthorityError("Workflow ref is incorrect.")
    if not isinstance(workflow["authority_commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", workflow["authority_commit"]):
        raise AuthorityError("Workflow authority commit is incorrect.")
    for name in ("run_id", "run_attempt"):
        if type(workflow[name]) is not int or workflow[name] < 1:
            raise AuthorityError(f"Workflow {name} is incorrect.")
    build = _exact_keys(record["build"], BUILD_KEYS, "Build")
    if (
        not isinstance(build["builder"], str)
        or re.fullmatch(r"github\.com/docker/buildx v\d+\.\d+\.\d+(?: .+)?", build["builder"]) is None
        or build["platform"] != "linux/amd64"
        or build["exporter"] != "oci"
        or build["provenance"] != "disabled"
        or build["compression"] != "gzip"
        or build["force_compression"] is not True
    ):
        raise AuthorityError("Build identity is incorrect.")
    if build["arguments"] != []:
        raise AuthorityError("Build arguments must be empty.")
    archive = _exact_keys(record["archive"], ARCHIVE_KEYS, "Archive")
    if not isinstance(archive["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", archive["sha256"]) or type(archive["size_bytes"]) is not int or archive["size_bytes"] < 1:
        raise AuthorityError("Archive identity is incorrect.")
    oci = _exact_keys(record["oci"], OCI_KEYS, "OCI")
    for name in ("archive_index_digest",):
        if not isinstance(oci[name], str) or not re.fullmatch(r"[0-9a-f]{64}", oci[name]):
            raise AuthorityError(f"OCI {name} is incorrect.")
    for name in (
        "archive_index_size_bytes", "platform_manifest_size_bytes",
        "image_configuration_size_bytes",
    ):
        if type(oci[name]) is not int or oci[name] < 1:
            raise AuthorityError(f"OCI {name} is incorrect.")
    _digest(oci["platform_manifest_digest"], "OCI platform manifest digest")
    _digest(oci["image_configuration_digest"], "OCI image configuration digest")
    if not isinstance(oci["layer_digests"], list) or not oci["layer_digests"]:
        raise AuthorityError("OCI layer digest list is incorrect.")
    for digest in oci["layer_digests"]:
        _digest(digest, "OCI layer digest")
    return record
