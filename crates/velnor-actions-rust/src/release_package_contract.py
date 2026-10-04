"""One closed immutable package schema and registry receipt relation."""
import re


PACKAGE_PROOF_FIELDS = {"files", "features", "archive_sha256", "publish_metadata",
                        "dependencies", "cargo_dependency_proofs", "forge_release"}


def features(value):
    require(isinstance(value, dict), "feature_map")
    result = {}
    for key, members in value.items():
        require(isinstance(key, str) and isinstance(members, list) and
                all(isinstance(member, str) for member in members), "feature_metadata")
        result[key] = sorted(set(members))
    return result


def _validate_package_files(files):
    require(isinstance(files, dict), "publish_package_files")
    for path, record in files.items():
        require(isinstance(path, str) and isinstance(record, dict) and
                set(record) == {"sha256", "size"} and isinstance(record["sha256"], str) and
                re.fullmatch(r"[0-9a-f]{64}", record["sha256"]) and
                type(record["size"]) is int and record["size"] >= 0, "publish_package_file")


def validate_package_shape(package):
    require(isinstance(package, dict) and set(package) == PACKAGE_PROOF_FIELDS,
            "publish_package_fields")
    require(isinstance(package["archive_sha256"], str) and
            re.fullmatch(r"[0-9a-f]{64}", package["archive_sha256"]), "publish_package_checksum")
    _validate_package_files(package["files"])
    require(isinstance(package["dependencies"], list) and
            all(isinstance(value, str) for value in package["dependencies"]),
            "publish_package_dependencies")
    require(isinstance(package["cargo_dependency_proofs"], list) and
            isinstance(package["forge_release"], dict), "publish_package_descriptors")
    features(package["features"])


def validate_registry_operation(operation, package, owners):
    validate_package_shape(package)
    require(isinstance(operation, dict) and set(operation) == {
        "version", "status", "candidate_archive_sha256", "relation", "registry"} and
            operation["status"] == "verified", "registry_operation_fields")
    require(operation["candidate_archive_sha256"] == package["archive_sha256"] and
            operation["version"] == package["publish_metadata"]["vers"],
            "registry_operation_candidate")
    proof = operation["registry"]
    require(isinstance(proof, dict) and set(proof) == {
        "status", "registry_checksum", "archive_checksum", "owners", "files", "features"
    } and proof["status"] == "verified", "registry_operation_proof")
    _validate_package_files(proof["files"])
    require(isinstance(proof["registry_checksum"], str) and
            re.fullmatch(r"[0-9a-f]{64}", proof["registry_checksum"]) and
            proof["registry_checksum"] == proof["archive_checksum"] and
            proof["owners"] == owners and proof["files"] == package["files"] and
            features(proof["features"]) == features(package["features"]),
            "registry_operation_content")
    relation = operation["relation"]
    require(relation in ("existing-normalized", "submitted-exact"), "registry_operation_relation")
    if relation == "submitted-exact":
        require(proof["registry_checksum"] == package["archive_sha256"],
                "registry_operation_submitted_checksum")
