"""Checkout-free publication of immutable Cargo package bytes."""
import os
import tarfile


def _check_missing_version(approved, name):
    crate = fetch(f"https://crates.io/api/v1/crates/{name}", 2 * 1024 * 1024)
    if crate is None:
        require(False, "publish_bootstrap_principal_unproven")
        return
    owners = []
    for kind, plural in (("user", "users"), ("team", "teams")):
        raw = fetch(f"https://crates.io/api/v1/crates/{name}/owner_{kind}", 2 * 1024 * 1024)
        require(raw is not None, "publish_existing_owners_missing")
        records = decode_json(raw).get(plural)
        require(isinstance(records, list), "publish_existing_owners_shape")
        for record in records:
            require(isinstance(record, dict) and type(record.get("id")) is int and
                    record["id"] > 0, "publish_existing_owner_identity")
            owners.append(f"{kind}:{record['id']}")
    require(sorted(owners) == approved["owners"][name], "publish_existing_owner_mismatch")


def _publish_one(approved, name, expected, archive, receipt):
    version = approved["packages"][name]
    operation = receipt["operations"][name]
    operation["candidate_archive_sha256"] = expected["archive_sha256"]
    existing = fetch(f"https://crates.io/api/v1/crates/{name}/{version}", 2 * 1024 * 1024)
    if existing is not None:
        operation["registry"] = verify_published_package(approved, name, version, expected)
        operation["relation"] = "existing-normalized"
        operation["status"] = "verified"
        validate_registry_operation(operation, expected, approved["owners"][name])
        return
    _check_missing_version(approved, name)
    operation["status"] = "uploading"
    save_receipt(receipt, "release-registry/receipt.json")
    with registry_token(approved, name) as token:
        try:
            upload_package(expected["publish_metadata"], archive, token)
        except ReconcileError:
            # Independent complete registry proof is the recovery authority.
            operation["status"] = "upload-uncertain"
        else:
            operation["status"] = "submitted"
        save_receipt(receipt, "release-registry/receipt.json")
    save_receipt(receipt, "release-registry/receipt.json")
    operation["registry"] = verify_published_package(approved, name, version, expected)
    require(operation["registry"]["registry_checksum"] == expected["archive_sha256"],
            "publish_submitted_checksum_mismatch")
    operation["relation"] = "submitted-exact"
    operation["status"] = "verified"
    validate_registry_operation(operation, expected, approved["owners"][name])


def publish_registry(approved, packages, archives, previous_receipt=None):
    order = validate_registry_inputs(approved, packages, archives)
    if previous_receipt is not None:
        require(isinstance(previous_receipt, dict) and previous_receipt.get("schema") == 1 and
                type(previous_receipt["schema"]) is int and
                same_json(previous_receipt.get("policy"), approved), "publish_recovery_authority")
    receipt = {"schema": 1, "policy": approved, "status": "incomplete",
               "workflow_sha": os.environ["GITHUB_SHA"], "run_id": os.environ["GITHUB_RUN_ID"],
               "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"], "publication_order": order,
               "operations": {name: {"version": version, "status": "pending"}
                              for name, version in approved["packages"].items()}}
    save_receipt(receipt, "release-registry/receipt.json")
    for name in order:
        operation = receipt["operations"][name]
        try:
            _publish_one(approved, name, packages[name], archives[name], receipt)
        except (ReconcileError, OSError, ValueError, TypeError, KeyError, tarfile.TarError) as error:
            operation["status"] = "failed"
            operation["reason"] = type(error).__name__
            save_receipt(receipt, "release-registry/receipt.json")
            return receipt
        save_receipt(receipt, "release-registry/receipt.json")
    receipt["status"] = "verified"
    save_receipt(receipt, "release-registry/receipt.json")
    return receipt
