"""Prepared claims are closed data; pure snapshots never become authority."""
import copy
import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1] / "src"
COMMON = ROOT.parents[1] / "velnor-actions-orchestrator" / "src"
FIXTURE = {"__file__": str(Path(__file__).with_name("release_publish_test_fixture.py"))}
exec(compile(Path(FIXTURE["__file__"]).read_text(), "package_fixture", "exec"), FIXTURE)
TREE_FIXTURE = {"__name__": "contract_tree_fixture"}
TREE_TEST = COMMON.parent / "tests/release_source_tree_test.py"
TREE_FIXTURE["__file__"] = str(TREE_TEST)
exec(compile(TREE_TEST.read_text(), str(TREE_TEST), "exec"), TREE_FIXTURE)


def fixture():
    tree = TREE_FIXTURE["SourceTreeTest"]()
    tree.setUp()
    ns = tree.ns
    sources = (COMMON / "release_reconcile_common.py", COMMON / "release_source_snapshot.py",
               COMMON / "release_source_artifact_input.py", ROOT / "release_package_contract.py",
               ROOT / "release_publish_metadata.py", ROOT / "release_publish_manifest.py",
               ROOT / "release_reconcile_cargo.py",
               COMMON / "release_forge_publish_verify.py", ROOT / "release_source_intent_contract.py")
    for source in sources:
        exec(compile(source.read_text(), str(source), "exec"), ns)
    source = tree.source()
    raw = ns["serialize_source_snapshot"](source)
    descriptor = {"repository": source.repository, "source_sha": source.source_sha,
        "tree_sha": source.tree_sha, "workflow": ".github/workflows/release.yml",
        "workflow_sha": "d" * 40, "ref": "refs/heads/main", "run_id": "123", "attempt": "1",
        "producer_job": "release-source-snapshot", "producer_helper_sha256": "a" * 64,
        "artifact_id": "456", "raw_zip_sha256": "b" * 64,
        "inner_sha256": hashlib.sha256(raw).hexdigest()}
    policy = {"schema": 1, "repository": source.repository, "registry": "crates-io",
        "source_sha": source.source_sha, "packages": {"demo": "1.0.0"},
        "owners": {"demo": ["user:1"]}, "tags": {"demo": "demo-v1.0.0"},
        "authentication": "bootstrap-token", "intent_id": "prepared-shape",
        "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")}}
    package, _archive = FIXTURE["package"]()
    evidence = {"schema": 1, "kind": "source-intent-prepared", "policy": policy,
                "source_snapshot": descriptor, "packages": {"demo": package},
                "publication_order": ["demo"]}
    snapshot = ns["decode_source_snapshot"](raw, policy)
    return ns, policy, descriptor, evidence, source, snapshot


class PreparedContractTests(unittest.TestCase):
    def setUp(self):
        self.ns, self.policy, self.descriptor, self.evidence, self.live, self.pure = fixture()

    def validate(self, evidence=None, descriptor=None):
        self.ns["validate_prepared_shape"](self.evidence if evidence is None else evidence,
            self.policy, self.descriptor if descriptor is None else descriptor)

    def rejected(self, change):
        evidence = copy.deepcopy(self.evidence)
        change(evidence)
        with self.assertRaises((self.ns["ReconcileError"], ValueError, TypeError)):
            self.validate(evidence)

    def test_closed_shape_is_valid_data_and_uses_actual_package_contract(self):
        self.validate()
        self.assertEqual(set(self.evidence), self.ns["PREPARED_EVIDENCE_FIELDS"])
        self.assertEqual(set(self.evidence["packages"]["demo"]), self.ns["PACKAGE_PROOF_FIELDS"])
        self.ns["validate_package_shape"](self.evidence["packages"]["demo"])
        self.assertEqual(self.evidence, json.loads(json.dumps(self.evidence)))

    def test_unknown_missing_kind_and_boolean_schema_are_rejected(self):
        for change in (lambda value: value.update(extra=True),
                       lambda value: value.pop("source_snapshot"),
                       lambda value: value.update(schema=True),
                       lambda value: value.update(kind="package-verified")):
            with self.subTest(change=change):
                self.rejected(change)

    def test_nested_policy_boolean_cannot_equal_integer_authority(self):
        self.rejected(lambda value: value["policy"].update(schema=True))
        self.rejected(lambda value: value["policy"].update(repository="other/repo"))
        self.rejected(lambda value: value["policy"]["tools"].update(rust="different"))
        self.rejected(lambda value: value["policy"]["owners"].update(demo=["user:2"]))

    def test_source_descriptor_drift_and_extra_destination_are_rejected(self):
        for key, changed in (("source_sha", "c" * 40), ("tree_sha", "c" * 40),
                             ("repository", "other/repo"), ("inner_sha256", "c" * 64),
                             ("artifact_id", "457"), ("attempt", "2"),
                             ("destination", "/caller-controlled")):
            with self.subTest(field=key):
                self.rejected(lambda value: value["source_snapshot"].update({key: changed}))

    def test_invalid_matching_claims_never_pass_descriptor_validation(self):
        for key, changed in (("run_id", True), ("artifact_id", "01"),
                             ("source_sha", "A" * 40), ("producer_job", "release-package"),
                             ("workflow", ".github/workflows/other.yml"),
                             ("inner_sha256", "A" * 64)):
            evidence = copy.deepcopy(self.evidence)
            evidence["source_snapshot"][key] = changed
            with self.subTest(field=key), self.assertRaises((self.ns["ReconcileError"], ValueError, TypeError)):
                self.validate(evidence, evidence["source_snapshot"])

    def test_internally_matching_source_claims_still_bind_policy(self):
        for key, changed in (("repository", "other/repo"), ("source_sha", "c" * 40)):
            evidence = copy.deepcopy(self.evidence)
            evidence["source_snapshot"][key] = changed
            with self.subTest(field=key), self.assertRaises(self.ns["ReconcileError"]):
                self.validate(evidence, evidence["source_snapshot"])

    def test_package_count_is_bounded_before_zip_publication(self):
        evidence, policy = copy.deepcopy(self.evidence), copy.deepcopy(self.policy)
        count = self.ns["PREPARED_MAX_PACKAGES"] + 1
        evidence["packages"] = {f"demo{index}": copy.deepcopy(evidence["packages"]["demo"])
                                for index in range(count)}
        policy["packages"] = {name: "1.0.0" for name in evidence["packages"]}
        evidence["policy"] = policy
        evidence["publication_order"] = sorted(evidence["packages"])
        with self.assertRaisesRegex(self.ns["ReconcileError"], "prepared_evidence_packages"):
            self.ns["validate_prepared_shape"](evidence, policy, self.descriptor)

    def test_scope_publication_order_and_dependency_proofs_are_rejected(self):
        for order in ([], ["other"], ["demo", "demo"]):
            self.rejected(lambda value: value.update(publication_order=order))
        self.rejected(lambda value: value["packages"].update(other=value["packages"]["demo"]))
        self.rejected(lambda value: value["packages"]["demo"].update(dependencies=["demo"]))
        self.rejected(lambda value: value["packages"]["demo"].update(extra=True))

    def test_invalid_metadata_checksum_files_and_forge_descriptor_are_rejected(self):
        self.rejected(lambda value: value["packages"]["demo"]["publish_metadata"].update(name="other"))
        self.rejected(lambda value: value["packages"]["demo"]["publish_metadata"].update(vers="2.0.0"))
        self.rejected(lambda value: value["packages"]["demo"].update(archive_sha256="bad"))
        self.rejected(lambda value: value["packages"]["demo"]["files"]["Cargo.toml"].update(size=True))
        self.rejected(lambda value: value["packages"]["demo"]["forge_release"].update(tag_name="other"))
        self.rejected(lambda value: value["packages"]["demo"]["forge_release"].update(make_latest=True))

    def dependency_claims(self):
        evidence = copy.deepcopy(self.evidence)
        package = evidence["packages"]["demo"]
        package["publish_metadata"]["deps"] = [{"name": "upstream", "version_req": "^1.0",
            "features": [], "optional": False, "default_features": True, "kind": "normal",
            "target": None, "explicit_name_in_toml": "alias"}]
        package["cargo_dependency_proofs"] = [{"kind": "normal", "target": None,
            "name_in_toml": "alias", "raw_requirement": "1.0", "canonical_requirement": "^1.0"}]
        return evidence

    def test_dependency_claims_require_exact_coverage_canonical_requirement_and_shape(self):
        valid = self.dependency_claims()
        self.validate(valid)
        changes = (lambda package: package.update(cargo_dependency_proofs=[{}]),
                   lambda package: package.update(cargo_dependency_proofs=[]),
                   lambda package: package["cargo_dependency_proofs"][0].update(canonical_requirement="^2.0"),
                   lambda package: package["cargo_dependency_proofs"][0].update(name_in_toml="other"),
                   lambda package: package["cargo_dependency_proofs"][0].update(extra=True),
                   lambda package: package["cargo_dependency_proofs"].append(package["cargo_dependency_proofs"][0]),
                   lambda package: package["publish_metadata"]["deps"].append(package["publish_metadata"]["deps"][0]))
        for change in changes:
            evidence = copy.deepcopy(valid)
            change(evidence["packages"]["demo"])
            with self.subTest(change=change), self.assertRaises(self.ns["ReconcileError"]):
                self.validate(evidence)
        self.rejected(lambda value: value["packages"]["demo"].update(
            cargo_dependency_proofs=valid["packages"]["demo"]["cargo_dependency_proofs"]))

    def test_pure_decoded_snapshot_live_api_tree_and_claims_grant_no_authority(self):
        for source in (self.pure, self.live, self.descriptor, Path("/source"), object()):
            with self.subTest(source=type(source).__name__), self.assertRaises(self.ns["ReconcileError"]):
                self.ns["validate_prepared_evidence"](self.evidence, self.policy, source)

    def test_standalone_actual_loader_remains_unqualified(self):
        with self.assertRaisesRegex(self.ns["ReconcileError"], "compiled_input_unqualified"):
            self.ns["load_authenticated_source_snapshot"]()


if __name__ == "__main__":
    unittest.main()
