"""Fresh public crates.io API, index and archive reconciliation."""
import hashlib
import re


def index_path(name):
    name = name.lower()
    if len(name) == 1:
        return "1/" + name
    if len(name) == 2:
        return "2/" + name
    if len(name) == 3:
        return "3/" + name[0] + "/" + name
    return name[:2] + "/" + name[2:4] + "/" + name


def registry_package(approved, name, version, expected):
    api = fetch(f"https://crates.io/api/v1/crates/{name}/{version}", 2 * 1024 * 1024)
    require(api is not None, "version_missing")
    record = decode_json(api).get("version", {})
    require(record.get("crate") == name and record.get("num") == version and
            record.get("yanked") is False, "version_identity_or_yanked")
    checksum = record.get("checksum")
    require(isinstance(checksum, str) and re.fullmatch(r"[0-9a-f]{64}", checksum), "registry_checksum")
    current_owners = []
    for kind, plural in (("user", "users"), ("team", "teams")):
        data = fetch(f"https://crates.io/api/v1/crates/{name}/owner_{kind}", 2 * 1024 * 1024)
        require(data is not None, "owners_missing")
        owners = decode_json(data).get(plural)
        require(isinstance(owners, list), "owners_shape")
        for owner in owners:
            identifier = owner.get("id") if isinstance(owner, dict) else None
            require(type(identifier) is int and identifier > 0, "owner_id")
            current_owners.append(f"{kind}:{identifier}")
    require(sorted(current_owners) == approved["owners"][name], "ownership_mismatch")
    raw_index = fetch("https://index.crates.io/" + index_path(name), 16 * 1024 * 1024)
    require(raw_index is not None, "index_missing")
    entries = [decode_json(line) for line in raw_index.splitlines() if line]
    selected = [entry for entry in entries if entry.get("vers") == version]
    require(bool(selected), "index_version_missing")
    require(len(selected) == 1, "index_version_duplicate")
    index = selected[0]
    require(index.get("name") == name and index.get("yanked") is False and
            index.get("cksum") == checksum, "index_registry_disagreement")
    combined = features(index.get("features", {}))
    for key, members in features(index.get("features2", {})).items():
        combined.setdefault(key, []).extend(members)
    require(features(combined) == features(expected["features"]) ==
            features(record.get("features")), "registry_feature_mismatch")
    data = fetch(f"https://static.crates.io/crates/{name}/{name}-{version}.crate")
    require(data is not None, "archive_missing")
    actual_checksum = hashlib.sha256(data).hexdigest()
    require(actual_checksum == checksum, "download_checksum_mismatch")
    actual = inventory(data, name, version, approved["source_sha"])
    require(actual["files"] == expected["files"] and
            features(actual["features"]) == features(expected["features"]), "source_content_mismatch")
    return {"status": "verified", "registry_checksum": checksum,
            "archive_checksum": actual_checksum, "owners": sorted(current_owners),
            "files": actual["files"], "features": actual["features"]}
