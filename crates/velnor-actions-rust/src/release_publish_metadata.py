"""Anonymous Cargo metadata bound to the exact approved package bytes."""
import tomllib


def _publish_dependency(dependency):
    require(dependency.get("registry") in (None, "https://github.com/rust-lang/crates.io-index"),
            "publish_dependency_registry")
    require(dependency.get("source") in (None, "registry+https://github.com/rust-lang/crates.io-index"),
            "publish_dependency_source")
    result = {
        "name": dependency["name"], "version_req": dependency["req"],
        "features": dependency["features"], "optional": dependency["optional"],
        "default_features": dependency["uses_default_features"],
        "target": dependency["target"], "kind": dependency["kind"] or "normal",
    }
    require(result["kind"] in ("normal", "build", "dev"), "publish_dependency_kind")
    if dependency.get("rename") is not None:
        result["explicit_name_in_toml"] = dependency["rename"]
    require(not dependency.get("artifact"), "publish_artifact_unsupported")
    return result


def _reject_artifact_dependencies(manifest):
    tables = [manifest, *manifest.get("target", {}).values()]
    for table in tables:
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            for dependency in table.get(kind, {}).values():
                require(not isinstance(dependency, dict) or not
                        set(dependency).intersection({"artifact", "lib", "target"}),
                        "publish_artifact_unsupported")


def _packaged_readme_file(package, contents):
    readme = package.get("readme")
    if readme is False:
        return None
    if readme is True:
        return "README.md"
    if readme is None:
        return next((name for name in ("README.md", "README.txt", "README") if name in contents), None)
    require(isinstance(readme, str), "publish_readme_path")
    return readme


def approved_publish_metadata(contents, metadata, name, version):
    manifest = tomllib.loads(contents["Cargo.toml"].decode("utf-8"))
    _reject_artifact_dependencies(manifest)
    matches = [package for package in metadata["packages"]
               if package["name"] == name and package["version"] == version]
    require(len(matches) == 1, "approved_metadata_identity")
    package = matches[0]
    normalized = manifest["package"]
    readme_file = _packaged_readme_file(normalized, contents)
    require(readme_file is None or isinstance(readme_file, str), "publish_readme_path")
    require(readme_file is None or readme_file in contents, "publish_readme_missing")
    output = {"name": name, "vers": version,
              "deps": [_publish_dependency(item) for item in package["dependencies"]],
              "features": manifest.get("features", {}),
              "authors": package["authors"], "keywords": package["keywords"],
              "categories": package["categories"], "badges": manifest.get("badges", {}),
              "readme_file": readme_file,
              "readme": None if readme_file is None else contents[readme_file].decode("utf-8")}
    for field in ("description", "documentation", "homepage", "repository", "license",
                  "license_file", "links", "rust_version"):
        output[field] = package.get(field)
    require(output["license_file"] == normalized.get("license-file"),
            "publish_license_metadata")
    require(output["license_file"] is None or output["license_file"] in contents,
            "publish_license_missing")
    validate_publish_metadata(output, name, version)
    return output


def _metadata_strings(value):
    return isinstance(value, list) and all(isinstance(item, str) for item in value)


def _validate_publish_dependency(dependency):
    fields = {"name", "version_req", "features", "optional", "default_features", "target", "kind"}
    require(isinstance(dependency, dict) and fields <= set(dependency) and
            set(dependency) <= fields | {"explicit_name_in_toml"}, "publish_dependency_shape")
    require(all(isinstance(dependency[key], str) and dependency[key]
                for key in ("name", "version_req", "kind")), "publish_dependency_strings")
    require(_metadata_strings(dependency["features"]) and
            type(dependency["optional"]) is bool and type(dependency["default_features"]) is bool,
            "publish_dependency_types")
    require(dependency["target"] is None or isinstance(dependency["target"], str),
            "publish_dependency_target")
    require(dependency["kind"] in ("normal", "build", "dev"), "publish_dependency_kind")
    require("explicit_name_in_toml" not in dependency or
            isinstance(dependency["explicit_name_in_toml"], str), "publish_dependency_rename")


def validate_publish_metadata(metadata, name, version):
    nullable = {"description", "documentation", "homepage", "readme", "readme_file",
                "license", "license_file", "repository", "links", "rust_version"}
    fields = nullable | {"name", "vers", "deps", "features", "authors", "keywords", "categories",
                         "badges"}
    require(isinstance(metadata, dict) and set(metadata) == fields, "publish_metadata_shape")
    require(metadata["name"] == name and metadata["vers"] == version, "publish_metadata_identity")
    require(all(metadata[key] is None or isinstance(metadata[key], str) for key in nullable),
            "publish_metadata_nullable")
    require(all(_metadata_strings(metadata[key]) for key in ("authors", "keywords", "categories")),
            "publish_metadata_lists")
    require(isinstance(metadata["features"], dict) and
            all(isinstance(key, str) and _metadata_strings(value)
                for key, value in metadata["features"].items()), "publish_metadata_features")
    require(isinstance(metadata["badges"], dict) and
            all(isinstance(key, str) and isinstance(value, dict) and
                all(isinstance(inner, str) and isinstance(text, str)
                    for inner, text in value.items())
                for key, value in metadata["badges"].items()), "publish_metadata_badges")
    require(isinstance(metadata["deps"], list), "publish_metadata_dependencies")
    for dependency in metadata["deps"]:
        _validate_publish_dependency(dependency)


def selected_publication_order(packages):
    remaining = set(packages)
    order = []
    while remaining:
        ready = sorted(name for name in remaining
                       if not remaining.intersection(packages[name]["dependencies"]))
        require(bool(ready), "selected_dependency_cycle")
        order.extend(ready)
        remaining.difference_update(ready)
    return order
