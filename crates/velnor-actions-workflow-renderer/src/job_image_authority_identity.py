# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Fixed publication identity and GitHub authorization checks.

Data-only port of the authorization logic in
``images/job/ubuntu-26.04/candidate/publish.py``. Every input arrives as
already-authenticated GitHub API data at the exact source SHA: the rendered
authority workflow performs the ``gh api`` and ``gh attestation verify``
calls, and this module only validates the returned payloads. It never reads
process state, never starts new tasks, never touches a checkout, and never
imports candidate code.
"""
import json
import re

REPOSITORY = "tailrocks/velnor-new"
REPOSITORY_ID = 1390620900
WORKFLOW = "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml"
ARTIFACT_PREFIX = "velnor-job-image-candidate-"
MAX_JSON_BYTES = 2 * 1024 * 1024
IDENTITY_KEYS = {
    "repository", "repository_id", "event", "workflow_ref", "ref",
    "authority_commit", "run_id", "run_attempt", "source", "artifact_id",
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


def _reject_constant(value):
    raise AuthorityError(f"JSON constant has an incorrect value: {value}.")


def _json_bytes(data, label):
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"{label} has an incorrect format.") from error


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


def _sha1(value, label):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise AuthorityError(f"{label} is incorrect.")
    return value


def _run_id(value, label):
    if type(value) is not int or value < 1:
        raise AuthorityError(f"{label} is incorrect.")
    return value


def load_identity(path):
    return _json_bytes(_read_limited_file(path, MAX_JSON_BYTES, "Publication identity"), "publication identity")


def validate_identity(value):
    if not isinstance(value, dict) or set(value) != IDENTITY_KEYS:
        raise AuthorityError("Publication identity has unknown or missing fields.")
    if value["repository"] != REPOSITORY:
        raise AuthorityError("Publication repository is incorrect.")
    if value["ref"] != "refs/heads/main" or value["event"] != "workflow_dispatch":
        raise AuthorityError("Publication is only for workflow_dispatch on refs/heads/main.")
    if value["workflow_ref"] != f"{WORKFLOW}@refs/heads/main":
        raise AuthorityError("Publication workflow authority is incorrect.")
    source = _sha1(value["source"], "Source SHA")
    authority = _sha1(value["authority_commit"], "Workflow authority SHA")
    if authority != source:
        raise AuthorityError("Workflow authority SHA is different from source SHA.")
    if type(value["repository_id"]) is not int or value["repository_id"] != REPOSITORY_ID:
        raise AuthorityError("GitHub repository ID is incorrect.")
    _run_id(value["run_id"], "Publication run ID")
    _run_id(value["run_attempt"], "Publication run attempt")
    _run_id(value["artifact_id"], "Candidate artifact ID")
    return value


def validate_repository(repository, tip, identity):
    if (
        not isinstance(repository, dict)
        or repository.get("full_name") != REPOSITORY
        or type(repository.get("id")) is not int
        or repository["id"] != REPOSITORY_ID
        or identity["repository_id"] != REPOSITORY_ID
    ):
        raise AuthorityError("GitHub repository ID is not the same as the canonical repository ID.")
    if not isinstance(tip, dict) or tip.get("sha") != identity["source"]:
        raise AuthorityError("Source SHA is not the current main commit.")
    return identity["source"]


def validate_candidate_identity(record, identity):
    source = record.get("source") if isinstance(record, dict) else None
    if (
        not isinstance(source, dict)
        or source.get("commit") != identity["source"]
        or source.get("ref") != "refs/heads/main"
    ):
        raise AuthorityError("Candidate source is not the current main source.")
    if record.get("workflow") != {
        "repository": REPOSITORY,
        "repository_id": identity["repository_id"],
        "event": "workflow_dispatch",
        "workflow_ref": identity["workflow_ref"],
        "authority_commit": identity["source"],
        "run_id": identity["run_id"],
        "run_attempt": identity["run_attempt"],
    }:
        raise AuthorityError("Candidate run identity is different from the publication run identity.")
    return record


def validate_artifact_record(value, identity):
    if not isinstance(value, dict) or not isinstance(value.get("artifacts"), list):
        raise AuthorityError("Candidate artifact response has an incorrect format.")
    expected_name = f"{ARTIFACT_PREFIX}{identity['run_attempt']}"
    matches = [
        item for item in value["artifacts"]
        if isinstance(item, dict) and item.get("name") == expected_name
    ]
    if len(matches) != 1:
        raise AuthorityError("This workflow run must have one candidate artifact.")
    artifact = matches[0]
    if (
        artifact.get("id") != identity["artifact_id"]
        or artifact.get("expired") is not False
        or artifact.get("workflow_run", {}).get("id") != identity["run_id"]
        or artifact.get("workflow_run", {}).get("head_sha") != identity["source"]
        or artifact.get("workflow_run", {}).get("head_branch") != "main"
    ):
        raise AuthorityError("Candidate artifact ID, expiry, or workflow run is incorrect.")
    return artifact["id"]


def select_ci_run(value, source):
    if not isinstance(value, dict) or not isinstance(value.get("workflow_runs"), list):
        raise AuthorityError("Canonical CI run response has an incorrect format.")
    runs = [
        run for run in value["workflow_runs"]
        if isinstance(run, dict)
        and run.get("path") == ".github/workflows/ci.yml"
        and run.get("event") == "push"
        and run.get("head_branch") == "main"
        and run.get("head_sha") == source
        and isinstance(run.get("head_repository"), dict)
        and run["head_repository"].get("full_name") == REPOSITORY
        and type(run.get("run_number")) is int
        and type(run.get("run_attempt")) is int
    ]
    if not runs:
        raise AuthorityError("No canonical main CI push run exists for this source.")
    runs.sort(key=lambda item: (item["run_number"], item["id"]))
    latest = runs[-1]
    same_number = [item for item in runs if item["run_number"] == latest["run_number"]]
    if len(same_number) != 1 or latest.get("status") != "completed" or latest.get("conclusion") != "success":
        raise AuthorityError("The latest canonical main CI run has no `success` conclusion.")
    return {"run_id": latest["id"], "run_attempt": latest["run_attempt"]}


def validate_attestation_output(value, source):
    if not isinstance(value, list) or not value:
        raise AuthorityError("Attestation verification output has no records.")
    for item in value:
        try:
            certificate = item["verificationResult"]["signature"]["certificate"]
            identifier = certificate["sourceRepositoryIdentifier"]
        except (KeyError, TypeError) as error:
            raise AuthorityError("Attestation certificate has no repository identifier.") from error
        if not (
            (type(identifier) is int and identifier == REPOSITORY_ID)
            or (type(identifier) is str and identifier == str(REPOSITORY_ID))
        ):
            raise AuthorityError("Attestation certificate repository ID is incorrect.")
        if (
            certificate.get("githubWorkflowRepository") != REPOSITORY
            or certificate.get("githubWorkflowSHA") != source
            or certificate.get("githubWorkflowRef") != "refs/heads/main"
            or certificate.get("githubWorkflowTrigger") != "workflow_dispatch"
            or certificate.get("sourceRepositoryRef") != "refs/heads/main"
            or certificate.get("sourceRepositoryDigest") != source
        ):
            raise AuthorityError("Attestation certificate source or workflow identity is incorrect.")
    return len(value)


def validate_source_inputs(record_source, authenticated_source):
    if not isinstance(authenticated_source, dict):
        raise AuthorityError("Authenticated source data has an incorrect format.")
    commit = authenticated_source.get("commit")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise AuthorityError("Authenticated source commit is incorrect.")
    if not isinstance(record_source, dict) or record_source.get("commit") != commit:
        raise AuthorityError("Candidate source is not the authenticated source.")
    inputs = authenticated_source.get("inputs")
    if not isinstance(inputs, dict) or not inputs:
        raise AuthorityError("Authenticated source inputs have an incorrect format.")
    for name, digest in inputs.items():
        if not isinstance(name, str) or not name or "\\" in name or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise AuthorityError("Source input hash is incorrect.")
    if record_source.get("inputs") != inputs:
        raise AuthorityError("Build input bytes changed after candidate creation.")
    return commit


def validate_build_lock(builder, buildx_version):
    if not isinstance(buildx_version, str) or re.fullmatch(r"\d+\.\d+\.\d+", buildx_version) is None:
        raise AuthorityError("Authenticated Buildx version is incorrect.")
    match = re.fullmatch(r"github\.com/docker/buildx v(\d+\.\d+\.\d+)(?: .+)?", builder)
    if match is None or match.group(1) != buildx_version:
        raise AuthorityError("Candidate Buildx identity is not the same as the value in .velnor/runner.lock.")
    return builder
