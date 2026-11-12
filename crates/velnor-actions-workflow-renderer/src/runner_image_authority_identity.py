# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import json
import re

REPOSITORY = "tailrocks/velnor-new"
REPOSITORY_ID = "1390620900"
REPOSITORY_URI = f"https://github.com/{REPOSITORY}"
CALLER = f"{REPOSITORY_URI}/.github/workflows/runner-image-qualification.yml@refs/heads/main"
SIGNER = f"{REPOSITORY_URI}/.github/workflows/runner-image-qualification-attest.yml@refs/heads/main"
RECEIPT_NAME = "RUNNER_IMAGE_QUALIFICATION.json"
RECEIPT_KEYS = {
    "schema", "source_commit", "source_ref", "runner_lock_sha256", "producer_run_id",
    "producer_run_attempt", "qualifier_run_id", "qualifier_run_attempt", "qualifier_job_id", "images",
}


class AuthorityError(ValueError):
    pass


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError("Duplicate JSON key.")
        result[key] = value
    return result


def _reject_constant(value):
    raise AuthorityError("Non-finite JSON constant.")


def parse_document(raw, limit):
    if not isinstance(raw, bytes) or not 0 < len(raw) <= limit:
        raise AuthorityError("Authority document exceeds its size limit.")
    try:
        value = json.loads(raw, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise AuthorityError("Authority document is invalid JSON.") from error
    pending = [(value, 0)]
    while pending:
        item, depth = pending.pop()
        if isinstance(item, (dict, list)):
            if depth >= 128:
                raise AuthorityError("Authority document exceeds its nesting limit.")
            values = item.values() if isinstance(item, dict) else item
            pending.extend((child, depth + 1) for child in values)
    return value


def _hex(value, length):
    return isinstance(value, str) and re.fullmatch(f"[0-9a-f]{{{length}}}", value) is not None


def _positive(value):
    return type(value) is int and 0 < value <= 2**64 - 1


def _certificate(source, run_id, attempt):
    return {
        "issuer": "https://token.actions.githubusercontent.com",
        "subjectAlternativeName": SIGNER,
        "sourceRepositoryURI": REPOSITORY_URI,
        "sourceRepositoryOwnerURI": "https://github.com/tailrocks",
        "sourceRepositoryIdentifier": REPOSITORY_ID,
        "sourceRepositoryDigest": source,
        "sourceRepositoryRef": "refs/heads/main",
        "buildSignerURI": SIGNER,
        "buildSignerDigest": source,
        "buildConfigURI": CALLER,
        "buildConfigDigest": source,
        "buildTrigger": "push",
        "runInvocationURI": f"{REPOSITORY_URI}/actions/runs/{run_id}/attempts/{attempt}",
        "runnerEnvironment": "github-hosted",
    }


def verify_qualification_attestation(raw, output, source, run_id, attempt):
    if not _hex(source, 40) or not _positive(run_id) or not _positive(attempt):
        raise AuthorityError("Qualification authority identity is invalid.")
    expected = _certificate(source, run_id, attempt)
    records = parse_document(output, 2 * 1024**2)
    if not isinstance(records, list) or not records:
        raise AuthorityError("Qualification attestation has no verified records.")
    matches = []
    for record in records:
        try:
            result = record["verificationResult"]
            certificate = result["signature"]["certificate"]
            invocation = certificate["runInvocationURI"]
        except (KeyError, TypeError) as error:
            raise AuthorityError("Qualification attestation has no invocation identity.") from error
        if invocation != expected["runInvocationURI"]:
            continue
        if any(certificate.get(key) != value for key, value in expected.items()):
            raise AuthorityError("Qualification certificate identity does not match.")
        try:
            statement = result["statement"]
            event = statement["predicate"]["buildDefinition"]["internalParameters"]["github"]["event_name"]
        except (KeyError, TypeError) as error:
            raise AuthorityError("Qualification statement has no event identity.") from error
        if statement.get("predicateType") != "https://slsa.dev/provenance/v1" or event != "push":
            raise AuthorityError("Qualification statement predicate or event does not match.")
        if statement.get("subject") != [{"name": RECEIPT_NAME, "digest": {"sha256": hashlib.sha256(raw).hexdigest()}}]:
            raise AuthorityError("Qualification receipt subject does not match its bytes.")
        matches.append(record)
    if len(matches) != 1:
        raise AuthorityError("Qualification attestation is missing or ambiguous.")


def authenticate_qualification_identity(raw, output, source, producer_id, producer_attempt, qualifier_id, qualifier_attempt, job_id):
    identities = {
        "producer_run_id": producer_id,
        "producer_run_attempt": producer_attempt,
        "qualifier_run_id": qualifier_id,
        "qualifier_run_attempt": qualifier_attempt,
        "qualifier_job_id": job_id,
    }
    if not all(_positive(value) for value in identities.values()) or producer_id == qualifier_id:
        raise AuthorityError("Qualification run identities are invalid.")
    if producer_attempt > 2**32 - 1 or qualifier_attempt > 2**32 - 1:
        raise AuthorityError("Qualification run attempt is too large.")
    receipt = parse_document(raw, 1024**2)
    if not isinstance(receipt, dict) or set(receipt) != RECEIPT_KEYS:
        raise AuthorityError("Qualification receipt has unknown or missing fields.")
    if (
        receipt["schema"] != "velnor.runner-image-qualification.v2"
        or receipt["source_commit"] != source
        or receipt["source_ref"] != "refs/heads/main"
        or not _hex(receipt["runner_lock_sha256"], 64)
        or any(type(receipt[key]) is not int or receipt[key] != value for key, value in identities.items())
        or not isinstance(receipt["images"], dict)
        or set(receipt["images"]) != {"runner", "dind", "resource_probe"}
    ):
        raise AuthorityError("Qualification receipt identity does not match.")
    verify_qualification_attestation(raw, output, source, qualifier_id, qualifier_attempt)
    return receipt
