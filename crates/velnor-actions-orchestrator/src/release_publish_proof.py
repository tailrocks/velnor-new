"""Authenticated immutable package and terminal receipt inputs for publishers."""
import os
from pathlib import Path
import re
import zipfile


def _release_artifact_name(prefix):
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    require(re.fullmatch(r"[1-9][0-9]*", run_id) and
            re.fullmatch(r"[1-9][0-9]*", attempt), "publish_artifact_run")
    return f"velnor-release-{prefix}-r{run_id}-a{attempt}"


def load_package_input(approved):
    """Verify authenticated ZIP, normalized Cargo archives, and publish metadata."""
    _artifact_upload_binding("release-package")
    blob, artifact = artifact_bytes(approved, _release_artifact_name("package"), "release-package")
    candidate, archives = validate_package_artifact(blob, approved)
    return candidate, archives, artifact, blob


def _receipt_member(blob):
    import io
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        members = archive.infolist()
        require(len(members) == 1 and members[0].filename == "receipt.json" and
                not members[0].is_dir() and members[0].file_size <= 16 * 1024 * 1024 and
                (members[0].external_attr >> 16) & 0o170000 in (0, 0o100000), "publish_receipt_members")
        return decode_json(archive.read(members[0]))


def load_publish_receipt(approved, kind):
    """Read one exact current-attempt terminal receipt from its fixed producer."""
    producers = {"registry": "release-registry-publish", "forge": "release-forge-publish"}
    require(kind in producers, "publish_receipt_kind")
    producer = producers[kind]
    _artifact_upload_binding(producer)
    blob, artifact = artifact_bytes(approved, _release_artifact_name(kind), producer)
    receipt = _receipt_member(blob)
    require(isinstance(receipt, dict) and set(receipt) == {
        "schema", "policy", "status", "workflow_sha", "run_id", "run_attempt",
        "publication_order", "operations"} and type(receipt.get("schema")) is int and receipt.get("schema") == 1 and
            same_json(receipt.get("policy"), approved) and receipt.get("workflow_sha") == os.environ["GITHUB_SHA"] and
            receipt.get("run_id") == os.environ["GITHUB_RUN_ID"] and
            receipt.get("run_attempt") == os.environ["GITHUB_RUN_ATTEMPT"], "publish_receipt_authority")
    if artifact["producer_job"]["conclusion"] != "success":
        require(receipt.get("status") == "incomplete", "failed_producer_receipt_status")
    operations = receipt.get("operations")
    require(isinstance(operations, dict) and set(operations) == set(approved["packages"]),
            "publish_receipt_scope")
    for name, version in approved["packages"].items():
        require(isinstance(operations[name], dict) and operations[name].get("version") == version,
                "publish_receipt_version")
    return receipt, artifact


def _registry_matches_candidate(receipt, candidate, approved):
    require(receipt["publication_order"] == candidate["publication_order"], "registry_publication_order")
    require(receipt.get("status") == "verified", "registry_publication_incomplete")
    for name, package in candidate["packages"].items():
        validate_registry_operation(receipt["operations"][name], package, approved["owners"][name])


def load_forge_publish_input(approved):
    candidate, _archives, package_artifact, _blob = load_package_input(approved)
    receipt, registry_artifact = load_publish_receipt(approved, "registry")
    require(registry_artifact["producer_job"]["conclusion"] == "success", "registry_producer_failed")
    _registry_matches_candidate(receipt, candidate, approved)
    for name, package in candidate["packages"].items():
        observed = verify_published_package(approved, name, approved["packages"][name], package)
        require(observed == receipt["operations"][name]["registry"], "registry_receipt_remote_changed")
    return candidate, {"package_artifact": package_artifact, "registry_artifact": registry_artifact}


def _verified_root():
    temporary = Path(os.environ["RUNNER_TEMP"])
    require(temporary.is_absolute() and temporary.resolve(strict=True) == temporary,
            "publish_temporary_root")
    parent = temporary / "velnor"
    require(not parent.is_symlink(), "publish_parent_symlink")
    parent.mkdir(mode=0o700, exist_ok=True)
    root = parent / "verified-release"
    require(not root.exists() and not root.is_symlink(), "publish_verified_root_exists")
    root.mkdir(mode=0o700)
    return root


def _write_verified_file(destination, blob):
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o400)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(blob)
        stream.flush()
        os.fsync(stream.fileno())


def create_registry_artifact_proof():
    approved = policy()
    candidate, archives, artifact, blob = load_package_input(approved)
    root = _verified_root()
    _write_verified_file(root / "artifact.zip", blob)
    for name, blob in archives.items():
        destination = root / f"{name}-{approved['packages'][name]}.crate"
        _write_verified_file(destination, blob)
    save_receipt(candidate, root / "evidence.json")
    save_receipt(artifact, root / "artifact.json")


def registry_artifact_proof_main():
    try:
        create_registry_artifact_proof()
    except (ReconcileError, OSError, ValueError, TypeError, KeyError, zipfile.BadZipFile) as error:
        raise SystemExit(f"release_registry_artifact_proof:{type(error).__name__}:{str(error)[:160]}") from error
