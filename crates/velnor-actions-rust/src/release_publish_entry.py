"""Fixed registry consumers read one authenticated immutable package buffer."""
import hashlib
import os
from pathlib import Path
import re


def _read_verified_artifact():
    artifact_id = os.environ["RELEASE_PACKAGE_ARTIFACT_ID"]
    digest = os.environ["RELEASE_PACKAGE_ARTIFACT_DIGEST"]
    require(re.fullmatch(r"[1-9][0-9]*", artifact_id) and
            re.fullmatch(r"[0-9a-f]{64}", digest), "registry_package_binding")
    temporary = Path(os.environ["RUNNER_TEMP"])
    root = temporary / "velnor" / "verified-release"
    require(temporary.is_absolute() and temporary.resolve(strict=True) == temporary and
            not (temporary / "velnor").is_symlink() and not root.is_symlink(),
            "registry_verified_namespace")
    path = root / "artifact.zip"
    require(path.is_file() and not path.is_symlink() and
            path.stat().st_size <= 256 * 1024 * 1024, "registry_verified_artifact")
    with path.open("rb") as stream:
        blob = stream.read(256 * 1024 * 1024 + 1)
    require(len(blob) <= 256 * 1024 * 1024 and
            hashlib.sha256(blob).hexdigest() == digest, "registry_verified_digest")
    identity_path = root / "artifact.json"
    require(identity_path.is_file() and not identity_path.is_symlink() and
            identity_path.stat().st_size <= 65536, "registry_verified_identity")
    with identity_path.open("rb") as stream:
        identity = decode_json(stream.read(65537))
    expected_name = f"velnor-release-package-r{os.environ['GITHUB_RUN_ID']}-a{os.environ['GITHUB_RUN_ATTEMPT']}"
    validate_artifact_identity(identity, int(artifact_id), "sha256:" + digest,
                               expected_name, "release-package", ("success",))
    return blob


def _registry_publish_main(authentication):
    try:
        approved = policy()
        require(approved["authentication"] == authentication, "registry_fixed_authentication")
        candidate, archives = validate_package_artifact(_read_verified_artifact(), approved)
        receipt = publish_registry(approved, candidate["packages"], archives)
        require(receipt["status"] == "verified", "registry_publication_incomplete")
    except (ReconcileError, OSError, ValueError, TypeError, KeyError, zipfile.BadZipFile) as error:
        raise SystemExit(f"registry_publish:{type(error).__name__}") from None


def bootstrap_publish_main():
    _registry_publish_main("bootstrap-token")


def trusted_publish_main():
    _registry_publish_main("trusted-publishing")
