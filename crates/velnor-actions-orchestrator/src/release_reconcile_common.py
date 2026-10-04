"""Fixed crates.io reconciliation primitives; no source-controlled imports."""
import json
import os
from pathlib import Path
import re


class ReconcileError(ValueError):
    """A closed release proof failed."""


def require(condition, reason):
    if not condition:
        raise ReconcileError(reason)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_json_key")
        result[key] = value
    return result


def decode_json(data):
    try:
        return json.loads(data, object_pairs_hook=unique_object,
                          parse_constant=lambda _: require(False, "nonfinite_json"))
    except ReconcileError:
        raise
    except (ValueError, UnicodeError, RecursionError, TypeError) as error:
        raise ReconcileError("json_invalid") from error


def same_json(value, expected):
    """Compare JSON authority without Python boolean/integer coercion."""
    try:
        return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False) == \
            json.dumps(expected, sort_keys=True, separators=(",", ":"), allow_nan=False)
    except ReconcileError:
        raise
    except (ValueError, UnicodeError, RecursionError, TypeError, OverflowError) as error:
        raise ReconcileError("json_invalid_comparison") from error


def policy():
    value = decode_json(os.environ["RELEASE_RECONCILE_POLICY"])
    require(isinstance(value, dict) and set(value) == {
        "schema", "repository", "registry", "source_sha", "packages", "owners",
        "tags", "authentication", "tools", "intent_id"}, "policy_fields")
    require(type(value["schema"]) is int and value["schema"] == 1 and value["registry"] == "crates-io", "policy_registry")
    require(re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", value["repository"]), "repository")
    require(re.fullmatch(r"[0-9a-f]{40}", value["source_sha"]), "source_sha")
    packages = value["packages"]
    require(isinstance(packages, dict) and packages, "empty_packages")
    require(set(packages) == set(value["owners"]) == set(value["tags"]), "policy_scope")
    for name, version in packages.items():
        require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", name), "package_name")
        require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version), "version")
        owners = value["owners"][name]
        require(isinstance(owners, list) and owners and owners == sorted(set(owners)), "owner_set")
        require(all(re.fullmatch(r"(?:user|team):[1-9][0-9]*", owner) for owner in owners), "owner_identity")
        require(re.fullmatch(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*", value["tags"][name]), "tag")
    require(value["authentication"] in ("trusted-publishing", "bootstrap-token"), "authentication")
    require(isinstance(value["tools"], dict) and set(value["tools"]) == {"generator", "release-plz", "rust", "python", "gh"}, "tools")
    require(isinstance(value["intent_id"], str) and re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", value["intent_id"]), "intent_id")
    return value


def validate_artifact_identity(identity, identifier, digest, name, producer_name, allowed_conclusions):
    """Closed authenticated artifact identity shared by fresh job consumers."""
    require(isinstance(identity, dict) and set(identity) == {
        "id", "digest", "name", "producer_job"}, "artifact_proof_fields")
    require(type(identifier) is int and identifier > 0 and type(identity["id"]) is int and
            isinstance(digest, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", digest) and
            isinstance(name, str) and name and identity["id"] == identifier and
            identity["digest"] == digest and identity["name"] == name, "artifact_proof_identity")
    producer = identity["producer_job"]
    require(isinstance(producer, dict) and set(producer) == {"id", "name", "conclusion"} and
            type(producer["id"]) is int and producer["id"] > 0 and
            producer["name"] == producer_name and
            producer["conclusion"] in allowed_conclusions, "artifact_proof_producer")
    return identity


def save_receipt(value, path="release-receipt/receipt.json"):
    destination = Path(path)
    require(not destination.is_symlink() and not destination.parent.is_symlink(), "receipt_symlink")
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(".tmp")
    require(not temporary.is_symlink(), "receipt_temp_symlink")
    with temporary.open("w", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True, separators=(",", ":"))
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(destination)
