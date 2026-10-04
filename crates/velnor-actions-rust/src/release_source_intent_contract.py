"""Closed prepared content claims; authenticated source authority stays separate."""

PREPARED_EVIDENCE_FIELDS = frozenset({
    "schema", "kind", "policy", "source_snapshot", "packages", "publication_order"})
PREPARED_MAX_ZIP_MEMBERS = 256
PREPARED_MAX_PACKAGES = PREPARED_MAX_ZIP_MEMBERS - 1
PREPARED_MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
PREPARED_MAX_TOTAL_ARCHIVE_BYTES = 256 * 1024 * 1024
PREPARED_MAX_EVIDENCE_BYTES = 16 * 1024 * 1024
PREPARED_MAX_ZIP_BYTES = (PREPARED_MAX_TOTAL_ARCHIVE_BYTES +
                          PREPARED_MAX_EVIDENCE_BYTES + 1024 * 1024)


def _validate_prepared_package_descriptors(package):
    proofs = _validated_dependency_proofs(package["cargo_dependency_proofs"])
    official = {}
    for dependency in package["publish_metadata"]["deps"]:
        key = (dependency["kind"], dependency["target"],
               dependency.get("explicit_name_in_toml", dependency["name"]))
        require(key not in official, "prepared_dependency_duplicate")
        official[key] = dependency["version_req"]
    require(set(proofs) == set(official), "prepared_dependency_proof_coverage")
    require(all(proofs[key]["canonical_requirement"] == requirement
                for key, requirement in official.items()), "prepared_dependency_canonical")


def validate_prepared_shape(evidence, approved, source_descriptor):
    """Validate closed content claims against supplied claims; grant no capability."""
    require(type(evidence) is dict and set(evidence) == PREPARED_EVIDENCE_FIELDS,
            "prepared_evidence_fields")
    require(type(evidence["schema"]) is int and evidence["schema"] == 1 and
            evidence["kind"] == "source-intent-prepared", "prepared_evidence_schema")
    require(same_json(evidence["policy"], approved), "prepared_evidence_policy")
    validate_source_snapshot_descriptor(source_descriptor)
    validate_source_snapshot_descriptor(evidence["source_snapshot"])
    require(same_json(evidence["source_snapshot"], source_descriptor),
            "prepared_evidence_source")
    require(source_descriptor["repository"] == approved["repository"] and
            source_descriptor["source_sha"] == approved["source_sha"],
            "prepared_evidence_source_policy")
    packages = evidence["packages"]
    require(type(packages) is dict and set(packages) == set(approved["packages"]) and
            0 < len(packages) <= PREPARED_MAX_PACKAGES, "prepared_evidence_packages")
    for name, version in approved["packages"].items():
        package = packages[name]
        validate_package_shape(package)
        validate_publish_metadata(package["publish_metadata"], name, version)
        _validate_prepared_package_descriptors(package)
        _descriptor(approved, name, package)
        require(same_json(package["features"], package["publish_metadata"]["features"]),
                "prepared_evidence_features")
        dependencies = sorted({item["name"] for item in package["publish_metadata"]["deps"]
                               if item["kind"] != "dev" and item["name"] in packages})
        require(same_json(package["dependencies"], dependencies),
                "prepared_evidence_dependencies")
    require(same_json(evidence["publication_order"], selected_publication_order(packages)),
            "prepared_evidence_order")


def validate_prepared_evidence(evidence, approved, source):
    """Bind content to a genuine authenticated original-source capability."""
    authenticated_source_snapshot(source, 16 * 1024 * 1024)
    descriptor = authenticated_source_descriptor(source)
    validate_prepared_shape(evidence, approved, descriptor)
