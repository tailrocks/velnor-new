// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) const DRIVER_A: &str = r#"import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(sys.argv[1])

sys.path.insert(0, str(ROOT))

import job_image_authority_identity as identity_module
import job_image_authority_publish as publish_module
import job_image_authority_registry as registry_module

ERRORS = (
    identity_module.AuthorityError,
    publish_module.AuthorityError,
    registry_module.AuthorityError,
)


def envelope():
    return json.loads(Path(sys.argv[3]).read_text())


def report(marker, detail):
    print(marker)
    if detail:
        print(detail)


def run_identity(case):
    try:
        identity_module.validate_identity(case)
    except ERRORS as error:
        report("IDENTITY-REJECTED", error)
        return
    report("IDENTITY-OK", case["source"])


def run_identity_file(case):
    try:
        value = identity_module.load_identity(Path(case["path"]))
        identity_module.validate_identity(value)
    except ERRORS as error:
        report("IDENTITY-REJECTED", error)
        return
    report("IDENTITY-OK", value["source"])


def run_repository(case):
    try:
        source = identity_module.validate_repository(case["repository"], case["tip"], case["identity"])
    except ERRORS as error:
        report("REPOSITORY-REJECTED", error)
        return
    report("REPOSITORY-OK", source)


def run_candidate_identity(case):
    try:
        identity_module.validate_candidate_identity(
            {"source": case["record_source"], "workflow": case["record_workflow"]},
            case["identity"],
        )
    except ERRORS as error:
        report("CANDIDATE-IDENTITY-REJECTED", error)
        return
    report("CANDIDATE-IDENTITY-OK", case["identity"]["source"])


def run_artifact(case):
    try:
        found = identity_module.validate_artifact_record(case["response"], case["identity"])
    except ERRORS as error:
        report("ARTIFACT-REJECTED", error)
        return
    report("ARTIFACT-OK", f"artifact_id={found}")


def run_ci(case):
    try:
        selected = identity_module.select_ci_run(case["listing"], case["source"])
    except ERRORS as error:
        report("CI-REJECTED", error)
        return
    report("CI-OK", f"run_id={selected['run_id']} run_attempt={selected['run_attempt']}")


def run_attestation(case):
    try:
        count = identity_module.validate_attestation_output(case["output"], case["source"])
    except ERRORS as error:
        report("ATTESTATION-REJECTED", error)
        return
    report("ATTESTATION-OK", f"records={count}")


def run_source_inputs(case):
    try:
        commit = identity_module.validate_source_inputs(case["record_source"], case["authenticated"])
    except ERRORS as error:
        report("SOURCE-INPUTS-REJECTED", error)
        return
    report("SOURCE-INPUTS-OK", commit)


def run_build_lock(case):
    try:
        identity_module.validate_build_lock(case["builder"], case["buildx_version"])
    except ERRORS as error:
        report("BUILD-LOCK-REJECTED", error)
        return
    report("BUILD-LOCK-OK", case["buildx_version"])


def run_registry_url(case):
    report("REGISTRY-URL-OK", registry_module.registry_url(case["path"]))


def run_download_location(case):
    try:
        target = registry_module.blob_download_location(case["location"], case["digest"])
    except ERRORS as error:
        report("DOWNLOAD-LOCATION-REJECTED", error)
        return
    report("DOWNLOAD-LOCATION-OK", target)


def run_upload_location(case):
    try:
        resolved = registry_module.validate_upload_location(case["base"], case["location"])
    except ERRORS as error:
        report("UPLOAD-LOCATION-REJECTED", error)
        return
    report("UPLOAD-LOCATION-OK", resolved)


def run_upload_target(case):
    try:
        target = registry_module.upload_target(case["location"], case["digest"])
    except ERRORS as error:
        report("UPLOAD-TARGET-REJECTED", error)
        return
    report("UPLOAD-TARGET-OK", target)


def run_challenge(case):
    try:
        params = registry_module.parse_auth_challenge(case["value"])
    except ERRORS as error:
        report("CHALLENGE-REJECTED", error)
        return
    report("CHALLENGE-OK", f"realm={params['realm']} service={params['service']} scope={params['scope']}")


def run_token_url(case):
    try:
        url = registry_module.token_request_url(case["params"])
    except ERRORS as error:
        report("TOKEN-URL-REJECTED", error)
        return
    report("TOKEN-URL-OK", url)


def run_token(case):
    try:
        registry_module.parse_token_payload(bytes.fromhex(case["payload_hex"]))
    except ERRORS as error:
        report("TOKEN-REJECTED", error)
        return
    report("TOKEN-OK", "shape")


def run_head(case):
    try:
        present = registry_module.check_head_response(
            case["status"], case["headers"], case["digest"], case["size"]
        )
    except ERRORS as error:
        report("HEAD-REJECTED", error)
        return
    report("HEAD-OK", f"present={present}")


def run_redirect(case):
    try:
        target = registry_module.redirect_target(case["path"], case["digest"], case["status"], case["location"])
    except ERRORS as error:
        report("REDIRECT-REJECTED", error)
        return
    if target is None:
        report("REDIRECT-NONE", "direct")
    else:
        report("REDIRECT-OK", target)


def run_get(case):
    try:
        size = registry_module.check_get_response(
            case["status"],
            case["headers"],
            bytes.fromhex(case["body_hex"]),
            case["digest"],
            case["size"],
            bytes.fromhex(case["expected_hex"]),
        )
    except ERRORS as error:
        report("GET-REJECTED", error)
        return
    report("GET-OK", f"size={size}")


def run_descriptors(case):
    try:
        unique = publish_module.blob_descriptors(case["manifest"])
    except ERRORS as error:
        report("DESCRIPTORS-REJECTED", error)
        return
    report("DESCRIPTORS-OK", f"digests={','.join(sorted(unique))}")


def run_manifest(case):
    try:
        raw = publish_module.read_manifest_bytes(Path(case["archive"]), case["digest"], case["size"])
    except ERRORS as error:
        report("MANIFEST-REJECTED", error)
        return
    report("MANIFEST-OK", f"size={len(raw)}")


def run_extract(case):
    try:
        with tempfile.TemporaryDirectory(prefix="gc1-t46-extract-") as temp:
            output = publish_module.extract_blob(
                Path(case["archive"]), case["digest"], case["size"], Path(temp)
            )
            data = Path(output).read_bytes()
    except ERRORS as error:
        report("EXTRACT-REJECTED", error)
        return
    import hashlib

    report("EXTRACT-OK", f"sha={hashlib.sha256(data).hexdigest()} size={len(data)}")


"#;
