"""Pure credential-boundary comparison of publish metadata to approved bytes."""
import json
import tomllib


def _proof_key(proof):
    return proof["kind"], proof["target"], proof["name_in_toml"]


def _validated_dependency_proofs(proofs):
    require(isinstance(proofs, list), "publish_dependency_proofs_type")
    fields = {"kind", "target", "name_in_toml", "raw_requirement", "canonical_requirement"}
    result = {}
    for proof in proofs:
        require(isinstance(proof, dict) and set(proof) == fields,
                "publish_dependency_proof_shape")
        require(all(isinstance(proof[key], str) and proof[key] for key in fields - {"target"}) and
                (proof["target"] is None or isinstance(proof["target"], str)) and
                proof["kind"] in ("normal", "build", "dev"), "publish_dependency_proof_types")
        key = _proof_key(proof)
        require(key not in result, "publish_dependency_proof_duplicate")
        result[key] = proof
    return result


def _archive_dependency(alias, specification, kind, target, proofs):
    if isinstance(specification, str):
        specification = {"version": specification}
    require(isinstance(specification, dict), "publish_manifest_dependency_shape")
    fields = {"version", "package", "features", "optional", "default-features", "registry-index"}
    require(set(specification) <= fields, "publish_manifest_dependency_unsupported")
    require(specification.get("registry-index") in
            (None, "https://github.com/rust-lang/crates.io-index"),
            "publish_manifest_dependency_registry")
    proof = proofs.get((kind, target, alias))
    require(proof is not None and proof["raw_requirement"] == specification.get("version"),
            "publish_dependency_proof_raw_requirement")
    dependency = {
        "name": specification.get("package", alias),
        "version_req": proof["canonical_requirement"],
        "features": specification.get("features", []),
        "optional": specification.get("optional", False),
        "default_features": specification.get("default-features", True),
        "kind": kind, "target": target,
    }
    if "package" in specification:
        dependency["explicit_name_in_toml"] = alias
    _validate_publish_dependency(dependency)
    return dependency


def _archive_dependency_tables(manifest):
    tables = [(None, manifest), *manifest.get("target", {}).items()]
    for target, table in tables:
        for heading, kind in (("dependencies", "normal"), ("build-dependencies", "build"),
                              ("dev-dependencies", "dev")):
            for alias, specification in table.get(heading, {}).items():
                yield alias, specification, kind, target


def archive_dependency_proofs(archive_contents, official_metadata):
    """Freeze Cargo's canonical requirements beside the exact manifest input."""
    manifest = tomllib.loads(archive_contents["Cargo.toml"].decode("utf-8"))
    official = {}
    for dependency in official_metadata["deps"]:
        key = (dependency["kind"], dependency["target"],
               dependency.get("explicit_name_in_toml", dependency["name"]))
        require(key not in official, "publish_dependency_proof_duplicate")
        official[key] = dependency
    proofs = []
    for alias, specification, kind, target in _archive_dependency_tables(manifest):
        raw = specification if isinstance(specification, str) else specification.get("version")
        key = (kind, target, alias)
        require(key in official and isinstance(raw, str), "publish_dependency_proof_coverage")
        proofs.append({"kind": kind, "target": target, "name_in_toml": alias,
                       "raw_requirement": raw, "canonical_requirement": official[key]["version_req"]})
    require(len(proofs) == len(official), "publish_dependency_proof_coverage")
    return sorted(proofs, key=lambda item: json.dumps(item, sort_keys=True))


def _archive_dependencies(manifest, proofs):
    mapping = _validated_dependency_proofs(proofs)
    dependencies = []
    for alias, specification, kind, target in _archive_dependency_tables(manifest):
        dependencies.append(_archive_dependency(alias, specification, kind, target, mapping))
    require(len(dependencies) == len(mapping), "publish_dependency_proof_coverage")
    return sorted(dependencies, key=lambda item: json.dumps(item, sort_keys=True))


def validate_archive_publish_metadata(archive_contents, metadata, name, version, proofs):
    validate_publish_metadata(metadata, name, version)
    require("Cargo.toml" in archive_contents, "publish_manifest_missing")
    manifest = tomllib.loads(archive_contents["Cargo.toml"].decode("utf-8"))
    package = manifest.get("package", {})
    require(package.get("name") == name and package.get("version") == version,
            "publish_manifest_identity")
    for field in ("description", "documentation", "homepage", "repository", "license", "links"):
        require(metadata[field] == package.get(field), "publish_manifest_field:" + field)
    for field in ("authors", "keywords", "categories"):
        require(metadata[field] == package.get(field, []), "publish_manifest_field:" + field)
    require(metadata["rust_version"] == package.get("rust-version"),
            "publish_manifest_field:rust_version")
    require(metadata["features"] == manifest.get("features", {}) and
            metadata["badges"] == manifest.get("badges", {}), "publish_manifest_features_badges")
    license_file = package.get("license-file")
    require(metadata["license_file"] == license_file and
            (license_file is None or license_file in archive_contents), "publish_manifest_license")
    readme_file = _packaged_readme_file(package, archive_contents)
    require(readme_file is None or isinstance(readme_file, str), "publish_manifest_readme_unsupported")
    require(metadata["readme_file"] == readme_file and
            (readme_file is None or readme_file in archive_contents), "publish_manifest_readme_file")
    readme = None if readme_file is None else archive_contents[readme_file].decode("utf-8")
    require(metadata["readme"] == readme, "publish_manifest_readme_contents")
    dependencies = sorted(metadata["deps"], key=lambda item: json.dumps(item, sort_keys=True))
    require(dependencies == _archive_dependencies(manifest, proofs), "publish_manifest_dependencies")
