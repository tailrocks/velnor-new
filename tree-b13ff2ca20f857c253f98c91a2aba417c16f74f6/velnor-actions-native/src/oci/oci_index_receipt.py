"""Strict REST transport and proof validation for immutable OCI indexes."""

import hashlib
import io
import json
import os
import re
import stat
import zipfile

from oci_digest import (
    GateError,
    api_json,
    auth_request,
    die,
    github_root,
    lower_sha,
    need,
    oci_version,
    output_values,
    required,
    safe_arch,
    safe_id,
    source_sha,
    verify_producer,
)
from oci_digest_parts import MAX_ARCHIVE, verify_artifact_run


def decimal(value, code):
    need(re.fullmatch(r"[1-9][0-9]{0,19}", value or "") is not None, code)
    return int(value)


def context():
    repository = required("REPOSITORY")
    need(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is not None, "receipt_repository")
    configured = os.environ.get("GITHUB_REPOSITORY")
    need(configured in (None, repository), "receipt_repository_binding")
    image, image_id = required("IMAGE"), required("IMAGE_ID")
    version, source = required("VERSION"), required("SOURCE_SHA")
    run_id, attempt = required("GITHUB_RUN_ID"), required("GITHUB_RUN_ATTEMPT")
    artifact_id, artifact_digest = required("ARTIFACT_ID"), required("ARTIFACT_DIGEST")
    job, platforms = required("ARTIFACT_JOB"), required("PLATFORMS").split(",")
    need(safe_id(image_id) and oci_version(version) and source_sha(source), "receipt_identity")
    need(lower_sha(artifact_digest), "receipt_artifact_digest")
    need(platforms and len(platforms) == len(set(platforms)) and all(safe_arch(item) for item in platforms), "receipt_platforms")
    need(job == "image-" + image_id, "receipt_artifact_job")
    return {
        "repository": repository,
        "image": image,
        "image_id": image_id,
        "version": version,
        "source": source,
        "run": decimal(run_id, "receipt_run"),
        "attempt": decimal(attempt, "receipt_attempt"),
        "artifact_id": decimal(artifact_id, "receipt_artifact_id"),
        "artifact_digest": artifact_digest,
        "platforms": set(platforms),
    }


def artifact_metadata(ctx, artifact, repository_id):
    expected_name = f"oci-index-{ctx['run']}-{ctx['attempt']}-{ctx['image_id']}"
    need(artifact.get("id") == ctx["artifact_id"] and artifact.get("name") == expected_name, "receipt_artifact_identity")
    need(artifact.get("expired") is False, "receipt_artifact_expired")
    size = artifact.get("size_in_bytes")
    need(type(size) is int and 0 < size <= MAX_ARCHIVE, "receipt_artifact_size")
    need(artifact.get("digest") == ctx["artifact_digest"], "receipt_artifact_digest")
    workflow = artifact.get("workflow_run")
    need(isinstance(workflow, dict) and workflow.get("id") == ctx["run"], "receipt_artifact_run")
    if "run_attempt" in workflow:
        need(workflow.get("run_attempt") == ctx["attempt"], "receipt_artifact_attempt")
    need(workflow.get("head_sha") == ctx["source"], "receipt_artifact_source")
    need(workflow.get("repository_id") == repository_id and workflow.get("head_repository_id") == repository_id, "receipt_artifact_repository")
    return size


def receipt_member(payload):
    try:
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            entries = archive.infolist()
            need(len(entries) == 1, "receipt_zip_entries")
            entry = entries[0]
            need(entry.filename == "index-proof.json" and entry.orig_filename == entry.filename, "receipt_zip_path")
            need(not entry.is_dir() and not entry.flag_bits & 1, "receipt_zip_file")
            mode = stat.S_IFMT(entry.external_attr >> 16)
            need(mode in (0, stat.S_IFREG) and entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED), "receipt_zip_type")
            need(entry.file_size <= 256 * 1024, "receipt_proof_size")
            proof = archive.read(entry)
            need(len(proof) == entry.file_size, "receipt_proof_size")
            return proof
    except (OSError, zipfile.BadZipFile) as error:
        raise GateError("receipt_zip") from error


def validate_proof(raw, ctx):
    try:
        proof = json.loads(raw.decode("utf-8"), object_pairs_hook=lambda pairs: duplicate_pairs(pairs))
    except (UnicodeDecodeError, ValueError, GateError) as error:
        raise GateError("receipt_proof_json") from error
    keys = {"schema", "image_id", "image", "version", "source_sha", "run_id", "run_attempt", "index_digest", "platform_digests"}
    need(isinstance(proof, dict) and set(proof) == keys, "receipt_proof_shape")
    need(type(proof["schema"]) is int and proof["schema"] == 1, "receipt_proof_schema")
    need(proof["image_id"] == ctx["image_id"] and proof["image"] == ctx["image"] and proof["version"] == ctx["version"], "receipt_proof_identity")
    need(proof["source_sha"] == ctx["source"] and proof["run_id"] == str(ctx["run"]) and proof["run_attempt"] == str(ctx["attempt"]), "receipt_proof_source")
    need(lower_sha(proof["index_digest"]), "receipt_index_digest")
    platforms = proof["platform_digests"]
    need(isinstance(platforms, dict) and set(platforms) == ctx["platforms"], "receipt_platform_set")
    need(all(lower_sha(value) for value in platforms.values()) and len(set(platforms.values())) == len(platforms), "receipt_platform_digests")
    canonical = json.dumps(proof, separators=(",", ":"), sort_keys=True).encode("utf-8")
    need(raw == canonical, "receipt_proof_bytes")
    return proof


def duplicate_pairs(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise GateError("receipt_duplicate_json_key")
        value[key] = item
    return value


def download_receipt():
    ctx, token = context(), required("GH_TOKEN")
    root = github_root(ctx["repository"])
    run_url = root + "/actions/runs/" + str(ctx["run"])
    repository_id = verify_artifact_run(ctx, api_json(token, run_url))
    verify_producer(token, ctx)
    artifact_url = root + "/actions/artifacts/" + str(ctx["artifact_id"])
    artifact = api_json(token, artifact_url)
    size = artifact_metadata(ctx, artifact, repository_id)
    payload = auth_request(token, artifact_url + "/zip", MAX_ARCHIVE, True)
    need(len(payload) == size and hashlib.sha256(payload).hexdigest() == ctx["artifact_digest"][7:], "receipt_archive_digest")
    need(verify_artifact_run(ctx, api_json(token, run_url)) == repository_id, "receipt_run_changed")
    second_size = artifact_metadata(ctx, api_json(token, artifact_url), repository_id)
    need(second_size == size, "receipt_artifact_changed")
    proof = validate_proof(receipt_member(payload), ctx)
    output_values({"index_digest": proof["index_digest"]})


def main():
    try:
        download_receipt()
    except GateError as error:
        die("index_receipt_" + str(error))
