"""Publish frozen forge descriptors without a checkout or registry client."""

import os
import re


FORGE_RECEIPT = "release-forge/receipt.json"
PACKAGE_EVIDENCE_FIELDS = {
    "schema", "policy", "packages", "publication_order", "workflow_sha",
    "run_id", "run_attempt", "status",
}
PROOF_FIELDS = {"package_artifact", "registry_artifact"}
MAX_WRITE_ATTEMPTS = 2


def _identity():
    source = os.environ.get("GITHUB_SHA", "")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    require(re.fullmatch(SHA, source) and re.fullmatch(r"[1-9][0-9]*", run_id) and
            re.fullmatch(r"[1-9][0-9]*", attempt), "forge_publish_identity")
    return source, run_id, attempt


def _validate_proofs(proofs):
    require(isinstance(proofs, dict) and set(proofs) == PROOF_FIELDS,
            "forge_publish_proofs")
    for key, prefix, producer in [
        ("package_artifact", "package", "release-package"),
        ("registry_artifact", "registry", "release-registry-publish"),
    ]:
        identifier, digest = _artifact_upload_binding(producer)
        validate_artifact_identity(
            proofs[key], identifier, digest, _release_artifact_name(prefix), producer,
            ("success",),
        )


def _candidate_values(approved, loaded):
    require(isinstance(loaded, tuple) and len(loaded) == 2, "forge_publish_input")
    candidate, proofs = loaded
    require(isinstance(candidate, dict) and set(candidate) == PACKAGE_EVIDENCE_FIELDS and
            candidate["schema"] == 1 and same_json(candidate["policy"], approved) and
            candidate["status"] == "package-verified" and
            candidate["workflow_sha"] == os.environ["GITHUB_SHA"] and
            candidate["run_id"] == os.environ["GITHUB_RUN_ID"] and
            candidate["run_attempt"] == os.environ["GITHUB_RUN_ATTEMPT"],
            "forge_publish_candidate")
    _validate_proofs(proofs)
    packages = candidate["packages"]
    order = candidate["publication_order"]
    require(isinstance(packages, dict) and set(packages) == set(approved["packages"]) and
            isinstance(order, list) and len(order) == len(packages) and
            set(order) == set(packages) and len(set(order)) == len(order),
            "forge_publish_package_scope")
    return candidate, order


def _candidate_input(approved):
    return _candidate_values(approved, load_forge_publish_input(approved))


def _validate_tag_response(approved, name, version, response):
    tag = approved["tags"][name]
    details = _tag_details(approved, tag, response)
    require(response["message"] == _tag_message(name, version), "forge_tag_message")
    return details


def _create_tag(approved, name, version):
    repository = approved["repository"]
    tag = approved["tags"][name]
    payload = {
        "tag": tag,
        "message": _tag_message(name, version),
        "object": approved["source_sha"],
        "type": "commit",
    }
    try:
        response = create_tag(repository, payload)
    except (ForgeWriteUncertain, ForgeWriteCollision):
        recovered = _tag_observation(approved, name, version)
        require(recovered is not None, "forge_tag_write_uncertain")
        return recovered
    try:
        details = _validate_tag_response(approved, name, version, response)
        fresh = read_tag_object(repository, details["object_sha"])
        fresh_details = _tag_details(approved, tag, fresh, details["object_sha"])
        require(fresh["message"] == _tag_message(name, version), "forge_tag_message")
        return fresh_details
    except ReconcileError:
        recovered = _tag_observation(approved, name, version)
        if recovered is not None:
            return recovered
        raise


def _validate_ref_response(tag, tag_object_sha, response):
    require(isinstance(response, dict) and response.get("ref") == "refs/tags/" + tag and
            isinstance(response.get("object"), dict) and
            response["object"].get("type") == "tag" and
            response["object"].get("sha") == tag_object_sha, "forge_ref_response")


def _ensure_tag(approved, name, version):
    observed = _tag_observation(approved, name, version)
    if observed is not None:
        return observed
    repository = approved["repository"]
    tag = approved["tags"][name]
    require(read_release(repository, tag) is None, "forge_release_without_tag")
    created = _create_tag(approved, name, version)
    tag_object_sha = created["object_sha"]
    ref_payload = {"ref": "refs/tags/" + tag, "sha": tag_object_sha}
    for attempt in range(MAX_WRITE_ATTEMPTS):
        try:
            response = create_tag_ref(repository, ref_payload)
        except (ForgeWriteUncertain, ForgeWriteCollision):
            recovered = _tag_observation(approved, name, version)
            if recovered is not None:
                return recovered
            if attempt == MAX_WRITE_ATTEMPTS - 1:
                raise ReconcileError("forge_ref_write_uncertain") from None
            continue
        try:
            _validate_ref_response(tag, tag_object_sha, response)
        except ReconcileError:
            recovered = _tag_observation(approved, name, version)
            if recovered is not None:
                return recovered
            if attempt == MAX_WRITE_ATTEMPTS - 1:
                raise ReconcileError("forge_ref_write_uncertain") from None
            continue
        recovered = _tag_observation(approved, name, version)
        if recovered is not None:
            return recovered
        if attempt == MAX_WRITE_ATTEMPTS - 1:
            raise ReconcileError("forge_ref_proof_missing")
    raise ReconcileError("forge_ref_proof_missing")


def _ensure_release(approved, descriptor):
    repository = approved["repository"]
    tag = descriptor["tag_name"]
    existing = read_release(repository, tag)
    if existing is not None:
        return _release_details(repository, descriptor, existing)
    for attempt in range(MAX_WRITE_ATTEMPTS):
        try:
            response = create_release(repository, descriptor)
        except (ForgeWriteUncertain, ForgeWriteCollision):
            recovered = read_release(repository, tag)
            if recovered is not None:
                return _release_details(repository, descriptor, recovered)
            if attempt == MAX_WRITE_ATTEMPTS - 1:
                raise ReconcileError("forge_release_write_uncertain") from None
            continue
        try:
            _release_details(repository, descriptor, response)
        except ReconcileError:
            recovered = read_release(repository, tag)
            if recovered is not None:
                return _release_details(repository, descriptor, recovered)
            raise
        recovered = read_release(repository, tag)
        if recovered is not None:
            return _release_details(repository, descriptor, recovered)
        if attempt == MAX_WRITE_ATTEMPTS - 1:
            raise ReconcileError("forge_release_proof_missing")
    raise ReconcileError("forge_release_proof_missing")


def _initial_receipt(approved, candidate, order):
    _source, run_id, attempt = _identity()
    return {
        "schema": 1,
        "policy": approved,
        "status": "incomplete",
        "workflow_sha": os.environ["GITHUB_SHA"],
        "run_id": run_id,
        "run_attempt": attempt,
        "publication_order": order,
        "operations": {
            name: {"version": approved["packages"][name], "status": "pending"}
            for name in candidate["packages"]
        },
    }


def publish_forge(approved, loaded=None):
    """Publish every approved descriptor and return its durable receipt."""
    if loaded is None:
        candidate, order = _candidate_input(approved)
    else:
        candidate, order = _candidate_values(approved, loaded)
    receipt = _initial_receipt(approved, candidate, order)
    save_receipt(receipt, FORGE_RECEIPT)
    for name in order:
        operation = receipt["operations"][name]
        version = operation["version"]
        try:
            descriptor = _descriptor(approved, name, candidate["packages"][name])
            operation["status"] = "tagging"
            save_receipt(receipt, FORGE_RECEIPT)
            operation["tag"] = _ensure_tag(approved, name, version)
            operation["status"] = "tag-verified"
            save_receipt(receipt, FORGE_RECEIPT)
            operation["status"] = "releasing"
            save_receipt(receipt, FORGE_RECEIPT)
            operation["release"] = _ensure_release(approved, descriptor)
            operation["status"] = "verified"
        except (ReconcileError, OSError, ValueError, TypeError, KeyError) as error:
            operation["status"] = "failed"
            operation["reason"] = type(error).__name__
            save_receipt(receipt, FORGE_RECEIPT)
            return receipt
        save_receipt(receipt, FORGE_RECEIPT)
    receipt["status"] = "verified"
    save_receipt(receipt, FORGE_RECEIPT)
    return receipt


def forge_publish_main():
    try:
        approved = policy()
        receipt = publish_forge(approved)
        require(receipt["status"] == "verified", "forge_publication_incomplete")
    except (ReconcileError, OSError, ValueError, TypeError, KeyError) as error:
        raise SystemExit(
            f"release_forge_publish:{type(error).__name__}:{str(error)[:160]}"
        ) from error


if __name__ == "__main__":
    forge_publish_main()
