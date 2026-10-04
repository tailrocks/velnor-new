"""Reconcile immutable package input and fresh public release proofs."""

import os
from pathlib import Path
import re
import subprocess
import tarfile
import zipfile


PACKAGE_CANDIDATE_FIELDS = {
    "schema", "policy", "packages", "publication_order", "workflow_sha",
    "run_id", "run_attempt", "status",
}
PREFLIGHT_FIELDS = PACKAGE_CANDIDATE_FIELDS | {"operations"}
PREFLIGHT_STATUS = "publication-incomplete"
TERMINAL_RECEIPT_FIELDS = {
    "schema", "policy", "status", "workflow_sha", "run_id", "run_attempt",
    "publication_order", "operations",
}


def _reconcile_identity():
    workflow_sha = os.environ.get("GITHUB_SHA", "")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    require(re.fullmatch(r"[0-9a-f]{40}", workflow_sha) and
            re.fullmatch(r"[1-9][0-9]*", run_id) and
            re.fullmatch(r"[1-9][0-9]*", attempt), "reconcile_run_identity")
    return workflow_sha, run_id, attempt


def _reconcile_initial_receipt(approved):
    workflow_sha, run_id, attempt = _reconcile_identity()
    return {
        "schema": 1,
        "policy": approved,
        "status": "incomplete",
        "workflow_sha": workflow_sha,
        "run_id": run_id,
        "run_attempt": attempt,
        "operations": {
            name: {"version": version, "status": "pending"}
            for name, version in approved["packages"].items()
        },
    }


def _validate_candidate(candidate, approved):
    require(isinstance(candidate, dict) and set(candidate) == PACKAGE_CANDIDATE_FIELDS and
            type(candidate["schema"]) is int and candidate["schema"] == 1 and
            same_json(candidate["policy"], approved) and
            candidate["status"] == "package-verified" and
            candidate["workflow_sha"] == os.environ["GITHUB_SHA"] and
            candidate["run_id"] == os.environ["GITHUB_RUN_ID"] and
            candidate["run_attempt"] == os.environ["GITHUB_RUN_ATTEMPT"],
            "reconcile_package_candidate")
    packages = candidate["packages"]
    order = candidate["publication_order"]
    require(isinstance(packages, dict) and set(packages) == set(approved["packages"]) and
            isinstance(order, list) and len(order) == len(packages) and
            set(order) == set(packages) and len(set(order)) == len(order) and
            all(isinstance(packages[name], dict) for name in order),
            "reconcile_package_scope")
    for name in order:
        validate_package_shape(packages[name])


def _validate_preflight(content, candidate, approved):
    evidence = decode_json(content) if isinstance(content, bytes) else content
    expected_operations = {
        name: {"version": version, "status": "pending"}
        for name, version in approved["packages"].items()
    }
    require(isinstance(evidence, dict) and set(evidence) == PREFLIGHT_FIELDS and
            type(evidence["schema"]) is int and evidence["schema"] == 1 and
            same_json(evidence["policy"], approved) and
            evidence["packages"] == candidate["packages"] and
            evidence["publication_order"] == candidate["publication_order"] and
            evidence["workflow_sha"] == os.environ["GITHUB_SHA"] and
            evidence["run_id"] == os.environ["GITHUB_RUN_ID"] and
            evidence["run_attempt"] == os.environ["GITHUB_RUN_ATTEMPT"] and
            evidence["status"] == PREFLIGHT_STATUS and
            evidence["operations"] == expected_operations,
            "reconcile_preflight_authority")


def _optional_preflight(approved, candidate, receipt):
    identifier = os.environ.get("RELEASE_PREFLIGHT_ARTIFACT_ID", "")
    digest = os.environ.get("RELEASE_PREFLIGHT_ARTIFACT_DIGEST", "")
    require(bool(identifier) == bool(digest), "preflight_artifact_binding")
    if not identifier:
        receipt["preflight_diagnostic"] = "artifact-not-produced"
        save_receipt(receipt)
        return None
    _workflow_sha, run_id, attempt = _reconcile_identity()
    name = f"velnor-release-preflight-r{run_id}-a{attempt}"
    content, artifact = artifact_evidence(approved, name, "release-preflight")
    _validate_artifact(artifact, "preflight", "release-preflight", ("success",))
    _validate_preflight(content, candidate, approved)
    return artifact


def _validate_artifact(artifact, prefix, producer, conclusions):
    _workflow_sha, run_id, attempt = _reconcile_identity()
    identifier, digest = _artifact_upload_binding(producer)
    name = f"velnor-release-{prefix}-r{run_id}-a{attempt}"
    return validate_artifact_identity(
        artifact, identifier, digest, name, producer, conclusions
    )


def _validate_terminal(receipt, artifact, kind, candidate, approved):
    producer = f"release-{kind}-publish"
    identity = _validate_artifact(
        artifact, kind, producer, ("success", "failure", "cancelled")
    )
    require(isinstance(receipt, dict) and
            set(receipt) == TERMINAL_RECEIPT_FIELDS and
            type(receipt.get("schema")) is int and receipt.get("schema") == 1 and
            same_json(receipt.get("policy"), approved) and
            receipt.get("workflow_sha") == os.environ["GITHUB_SHA"] and
            receipt.get("run_id") == os.environ["GITHUB_RUN_ID"] and
            receipt.get("run_attempt") == os.environ["GITHUB_RUN_ATTEMPT"] and
            receipt.get("publication_order") == candidate["publication_order"],
            f"{kind}_receipt_authority")
    status = receipt.get("status")
    require(status in ("incomplete", "verified"), f"{kind}_receipt_status")
    if identity["producer_job"]["conclusion"] != "success":
        require(status == "incomplete", f"{kind}_failed_producer_receipt")
    operations = receipt.get("operations")
    require(isinstance(operations, dict) and set(operations) == set(approved["packages"]) and
            all(isinstance(operations[name], dict) and
                operations[name].get("version") == approved["packages"][name]
                for name in approved["packages"]), f"{kind}_receipt_scope")
    if status == "verified":
        require(all(operations[name].get("status") == "verified"
                     for name in approved["packages"]), f"{kind}_receipt_status")
    if kind == "registry":
        for name, package in candidate["packages"].items():
            operation = operations[name]
            if operation.get("status") == "verified":
                validate_registry_operation(operation, package, approved["owners"][name])


def _reconcile_package(receipt, approved, candidate, name):
    version = approved["packages"][name]
    operation = receipt["operations"][name]
    operation["status"] = "incomplete"
    for field in ("reason", "registry", "forge"):
        operation.pop(field, None)
    save_receipt(receipt)
    failed = False
    try:
        operation["registry"] = verify_published_package(
            approved, name, version, candidate["packages"][name]
        )
    except (ReconcileError, OSError, ValueError, KeyError, TypeError,
            tarfile.TarError, zipfile.BadZipFile, subprocess.SubprocessError) as error:
        failed = True
        operation["reason"] = str(error)[:160]
    save_receipt(receipt)
    try:
        descriptor = candidate["packages"][name]["forge_release"]
        operation["forge"] = verify_forge_package(approved, name, descriptor)
    except (ReconcileError, OSError, ValueError, KeyError, TypeError,
            subprocess.SubprocessError) as error:
        failed = True
        operation.setdefault("reason", str(error)[:160])
    operation["status"] = "failed" if failed else "verified"
    save_receipt(receipt)
    return not failed


def reconcile():
    approved = policy()
    receipt = _reconcile_initial_receipt(approved)
    save_receipt(receipt)
    candidate, _archives, package_artifact, _raw_blob = load_package_input(approved)
    _validate_artifact(package_artifact, "package", "release-package", ("success",))
    _validate_candidate(candidate, approved)
    receipt["publication_order"] = candidate["publication_order"]
    receipt["package_artifact"] = package_artifact
    save_receipt(receipt)
    preflight_artifact = _optional_preflight(approved, candidate, receipt)
    if preflight_artifact is not None:
        receipt["preflight_artifact"] = preflight_artifact
        save_receipt(receipt)
    registry_receipt, registry_artifact = load_publish_receipt(approved, "registry")
    _validate_terminal(registry_receipt, registry_artifact, "registry", candidate, approved)
    receipt.update({"registry_receipt": registry_receipt,
                    "registry_artifact": registry_artifact})
    save_receipt(receipt)
    forge_receipt, forge_artifact = load_publish_receipt(approved, "forge")
    _validate_terminal(forge_receipt, forge_artifact, "forge", candidate, approved)
    receipt.update({"forge_receipt": forge_receipt, "forge_artifact": forge_artifact})
    save_receipt(receipt)
    complete = True
    for name in candidate["publication_order"]:
        complete = _reconcile_package(receipt, approved, candidate, name) and complete
    receipt["status"] = "verified" if complete else "incomplete"
    save_receipt(receipt)
    require(complete, "partial_or_failed_reconciliation")


def reconcile_main():
    try:
        reconcile()
    except (ReconcileError, OSError, ValueError, KeyError, TypeError,
            tarfile.TarError, zipfile.BadZipFile, subprocess.SubprocessError) as error:
        destination = Path("release-receipt/receipt.json")
        if not destination.exists():
            save_receipt({"schema": 1, "status": "incomplete", "operations": {},
                          "reason": type(error).__name__})
        raise SystemExit(f"release_reconcile:{type(error).__name__}:{str(error)[:160]}") from error


if __name__ == "__main__":
    reconcile_main()
