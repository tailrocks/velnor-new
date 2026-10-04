"""Credential consumer rederives every publish field from immutable archive bytes."""
from pathlib import Path
import unittest


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "publish_manifest_test", "require": require}
for source in ("release_publish_metadata.py", "release_publish_manifest.py"):
    exec(compile((ROOT / source).read_text(), source, "exec"), NS)


MANIFEST = b'''[package]
name = "demo"
version = "1.0.0"
description = "Approved description"
authors = ["Author"]
license = "MIT"
license-file = "LICENSE"
rust-version = "1.98"
readme = "README.md"
[features]
default = ["dep:alias"]
[dependencies.alias]
package = "actual"
version = "=2.0.0"
features = ["json"]
optional = true
default-features = false
[target.'cfg(unix)'.build-dependencies.tool]
version = "=3.0.0"
'''


def metadata():
    result = {key: None for key in ("description", "documentation", "homepage", "readme",
                                   "readme_file", "license", "license_file", "repository", "links",
                                   "rust_version")}
    result.update(name="demo", vers="1.0.0", description="Approved description", authors=["Author"],
                  keywords=[], categories=[], license="MIT", license_file="LICENSE", rust_version="1.98",
                  readme_file="README.md", readme="Approved README\n", features={"default": ["dep:alias"]},
                  badges={}, deps=[{"name": "actual", "version_req": "=2.0.0", "features": ["json"],
                                      "optional": True, "default_features": False, "target": None,
                                      "kind": "normal", "explicit_name_in_toml": "alias"},
                                     {"name": "tool", "version_req": "=3.0.0", "features": [],
                                      "optional": False, "default_features": True, "target": "cfg(unix)",
                                      "kind": "build"}])
    return result


class ManifestMetadataTests(unittest.TestCase):
    def setUp(self):
        self.contents = {"Cargo.toml": MANIFEST, "README.md": b"Approved README\n", "LICENSE": b"MIT"}
        self.proofs = NS["archive_dependency_proofs"](self.contents, metadata())

    def validate(self, candidate):
        NS["validate_archive_publish_metadata"](self.contents, candidate, "demo", "1.0.0", self.proofs)

    def test_exact_archive_metadata_accepted_regardless_dependency_order(self):
        candidate = metadata()
        candidate["deps"].reverse()
        self.validate(candidate)

    def test_each_scalar_and_collection_substitution_rejected(self):
        for field, value in (("description", "changed"), ("homepage", "https://evil.example"),
                             ("authors", ["Attacker"]), ("keywords", ["changed"]),
                             ("categories", ["changed"]), ("license", "Apache-2.0"),
                             ("license_file", None), ("repository", "https://evil.example"),
                             ("documentation", "https://evil.example"), ("links", "changed"),
                             ("rust_version", "1.99"), ("readme", "Changed README"),
                             ("readme_file", None), ("features", {}),
                             ("badges", {"maintenance": {"status": "deprecated"}})):
            candidate = metadata()
            candidate[field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "publish_manifest"):
                self.validate(candidate)

    def test_each_dependency_substitution_rejected(self):
        for field, value in (("name", "other"), ("version_req", "=9.0.0"), ("features", []),
                             ("optional", False), ("default_features", True), ("kind", "dev"),
                             ("target", "cfg(windows)"), ("explicit_name_in_toml", "wrong")):
            candidate = metadata()
            candidate["deps"][0][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "publish_manifest_dependencies"):
                self.validate(candidate)

    def test_false_readme_is_absent_and_license_must_exist(self):
        self.contents["Cargo.toml"] = MANIFEST.replace(b'readme = "README.md"', b'readme = false')
        candidate = metadata()
        candidate.update(readme=None, readme_file=None)
        self.validate(candidate)
        del self.contents["LICENSE"]
        with self.assertRaisesRegex(ValueError, "publish_manifest_license"):
            self.validate(candidate)

    def test_full_cargo_requirement_grammar_preserved_as_frozen_proof(self):
        cases = [("2", "^2"), ("^2.0.0", "^2.0.0"), (">=2.0, <3", ">=2.0, <3"),
                 ("2.*", "2.*"), ("2.0.0-beta.1+build", "^2.0.0-beta.1")]
        for raw, canonical in cases:
            with self.subTest(raw=raw):
                self.contents["Cargo.toml"] = MANIFEST.replace(b'version = "=2.0.0"',
                                                               ('version = "' + raw + '"').encode())
                candidate = metadata()
                candidate["deps"][0]["version_req"] = canonical
                self.proofs = NS["archive_dependency_proofs"](self.contents, candidate)
                self.validate(candidate)
                self.proofs[0]["raw_requirement"] = "=wrong"
                with self.assertRaisesRegex(ValueError, "proof_raw_requirement"):
                    self.validate(candidate)

    def test_proof_canonical_coverage_and_type_tampering_rejected(self):
        for edit in (lambda proofs: proofs[0].update(canonical_requirement="=9.0.0"),
                     lambda proofs: proofs.pop(), lambda proofs: proofs.append(dict(proofs[0])),
                     lambda proofs: proofs[0].update(raw_requirement=123)):
            self.proofs = NS["archive_dependency_proofs"](self.contents, metadata())
            edit(self.proofs)
            with self.assertRaisesRegex(ValueError, "publish_(dependency_proof|manifest_dependencies)"):
                self.validate(metadata())
        self.contents["Cargo.toml"] = MANIFEST.replace(b'optional = true',
                                                       b'optional = true\nartifact = "bin"')
        self.proofs = NS["archive_dependency_proofs"](self.contents, metadata())
        with self.assertRaisesRegex(ValueError, "unsupported"):
            self.validate(metadata())


if __name__ == "__main__":
    unittest.main()
