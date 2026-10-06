"""Read-only public registry metadata and immutable package verification."""
import json
import time


def _dependency_projection(metadata):
    return sorted(({
        "crate_id": dependency["name"], "req": dependency["version_req"],
        "optional": dependency["optional"], "default_features": dependency["default_features"],
        "features": sorted(dependency["features"]), "target": dependency["target"],
        "kind": dependency["kind"],
    } for dependency in metadata["deps"]), key=lambda value: json.dumps(value, sort_keys=True))


def _index_dependency(dependency):
    require(isinstance(dependency, dict), "publish_index_dependency")
    result = {"name": dependency.get("package") or dependency.get("name"),
              "version_req": dependency.get("req"), "features": sorted(dependency.get("features", [])),
              "optional": dependency.get("optional"),
              "default_features": dependency.get("default_features"),
              "target": dependency.get("target"), "kind": dependency.get("kind") or "normal"}
    if dependency.get("package") is not None:
        result["explicit_name_in_toml"] = dependency["name"]
    if dependency.get("registry") is not None:
        result["registry"] = dependency["registry"]
    _validate_publish_dependency(result)
    return result


def _verify_published_once(approved, name, version, expected):
    proof = registry_package(approved, name, version, expected)
    raw = fetch("https://index.crates.io/" + index_path(name), 16 * 1024 * 1024)
    require(raw is not None, "publish_index_missing")
    selected = [decode_json(line) for line in raw.splitlines()
                if line and decode_json(line).get("vers") == version]
    require(len(selected) == 1 and isinstance(selected[0].get("deps"), list),
            "publish_index_dependencies")
    index = [_index_dependency(dependency) for dependency in selected[0]["deps"]]
    expected_deps = [{**dependency, "features": sorted(dependency["features"])}
                     for dependency in expected["publish_metadata"]["deps"]]
    sort_key = lambda value: json.dumps(value, sort_keys=True)
    require(sorted(index, key=sort_key) == sorted(expected_deps, key=sort_key),
            "publish_index_dependency_mismatch")
    data = fetch(f"https://crates.io/api/v1/crates/{name}/{version}/dependencies", 2 * 1024 * 1024)
    require(data is not None, "publish_api_dependencies_missing")
    api = decode_json(data).get("dependencies")
    fields = {"crate_id", "req", "optional", "default_features", "features", "target", "kind"}
    require(isinstance(api, list) and all(isinstance(value, dict) and fields <= set(value)
                                       for value in api), "publish_api_dependencies")
    projected = [{key: sorted(value[key]) if key == "features" else value[key]
                  for key in fields} for value in api]
    for value in projected:
        _validate_publish_dependency({"name": value["crate_id"], "version_req": value["req"],
                **{key: value[key] for key in fields - {"crate_id", "req"}}})
    require(sorted(projected, key=sort_key) == _dependency_projection(expected["publish_metadata"]),
            "publish_api_dependency_mismatch")
    return proof


def verify_published_package(approved, name, version, expected):
    missing = {"version_missing", "index_missing", "index_version_missing", "archive_missing",
               "publish_api_dependencies_missing"}
    for delay in (0, 1, 2, 4, 8):
        if delay:
            time.sleep(delay)
        try:
            return _verify_published_once(approved, name, version, expected)
        except ReconcileError as error:
            if str(error) not in missing:
                raise
    raise ReconcileError("publish_visibility_exhausted")


