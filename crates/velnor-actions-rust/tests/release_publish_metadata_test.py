"""Pinned Cargo publish metadata and anonymous archive probe regressions."""
import io
import json
from pathlib import Path
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "publish_metadata_test", "require": require}
for source in ("release_publish_metadata.py", "release_publish_manifest.py", "release_preflight_cargo.py"):
    exec(compile((ROOT / source).read_text(), source, "exec"), NS)


def dependency(name="dependency", rename="alias"):
    return {"name": name, "req": "^2.0.0", "features": ["json"], "optional": True,
            "uses_default_features": False, "kind": None, "target": 'cfg(unix)',
            "registry": None, "source": "registry+https://github.com/rust-lang/crates.io-index",
            "rename": rename}


def package():
    return {"name": "demo", "version": "1.0.0", "dependencies": [dependency()],
            "authors": ["Example"], "keywords": ["ci"], "categories": ["development-tools"],
            "license": "MIT", "rust_version": "1.98"}


CONTENTS = {"Cargo.toml": b'''[package]
name = "demo"
version = "1.0.0"
readme = "README.md"
authors = ["Example"]
keywords = ["ci"]
categories = ["development-tools"]
license = "MIT"
rust-version = "1.98"
[target.'cfg(unix)'.dependencies.alias]
package = "dependency"
version = "2.0.0"
features = ["json"]
optional = true
default-features = false
[features]
default = ["dep:alias"]
[badges.maintenance]
status = "actively-developed"
''', "README.md": b"Exact README\n"}


class PublishMetadataTests(unittest.TestCase):
    def metadata(self):
        return NS["approved_publish_metadata"](CONTENTS, {"packages": [package()]}, "demo", "1.0.0")

    def test_exact_cargo_fields_rename_and_normalized_features(self):
        metadata = self.metadata()
        dep = metadata["deps"][0]
        self.assertEqual(dep["name"], "dependency")
        self.assertEqual(dep["explicit_name_in_toml"], "alias")
        self.assertEqual(dep["version_req"], "^2.0.0")
        self.assertEqual(dep["kind"], "normal")
        self.assertNotIn("registry", dep)
        self.assertNotIn("artifact", dep)
        self.assertEqual(metadata["features"], {"default": ["dep:alias"]})
        self.assertEqual(metadata["readme"], "Exact README\n")
        self.assertEqual(metadata["readme_file"], "README.md")
        self.assertEqual(metadata["badges"], {"maintenance": {"status": "actively-developed"}})
        self.assertIsNone(metadata["homepage"])

    def test_unknown_authority_and_wrong_types_rejected(self):
        for field, value in (("registry", "https://evil.example/index"),
                             ("source", "git+https://evil.example/repo")):
            dep = dependency()
            dep[field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "publish_dependency"):
                NS["_publish_dependency"](dep)
        for field, value in (("extra", True), ("deps", {}), ("authors", "Example")):
            metadata = self.metadata()
            metadata[field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "publish_metadata"):
                NS["validate_publish_metadata"](metadata, "demo", "1.0.0")

    def test_missing_readme_and_artifact_fail_closed(self):
        with self.assertRaisesRegex(ValueError, "publish_readme_missing"):
            NS["approved_publish_metadata"]({"Cargo.toml": CONTENTS["Cargo.toml"]},
                                             {"packages": [package()]}, "demo", "1.0.0")
        contents = dict(CONTENTS)
        contents["Cargo.toml"] += b'[dependencies.tool]\nversion = "1"\nartifact = "bin"\n'
        with self.assertRaisesRegex(ValueError, "publish_artifact_unsupported"):
            NS["approved_publish_metadata"](contents, {"packages": [package()]}, "demo", "1.0.0")

    def test_readme_boolean_and_automatic_selection_match_cargo(self):
        cases = [(b"readme = false", None), (b"readme = true", "README.md"), (b"", "README.md")]
        for declaration, expected in cases:
            contents = dict(CONTENTS)
            contents["Cargo.toml"] = CONTENTS["Cargo.toml"].replace(b'readme = "README.md"', declaration)
            with self.subTest(declaration=declaration):
                result = NS["approved_publish_metadata"](contents, {"packages": [package()]}, "demo", "1.0.0")
                self.assertEqual(result["readme_file"], expected)
                self.assertEqual(result["readme"], None if expected is None else "Exact README\n")

    def test_selected_order_deterministic_and_cycles_rejected(self):
        packages = {"top": {"dependencies": ["base"]}, "independent": {"dependencies": []},
                    "base": {"dependencies": []}}
        self.assertEqual(NS["selected_publication_order"](packages), ["base", "independent", "top"])
        packages["base"]["dependencies"] = ["top"]
        with self.assertRaisesRegex(ValueError, "selected_dependency_cycle"):
            NS["selected_publication_order"](packages)

    def test_probe_runs_only_anonymous_no_dependency_resolution(self):
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w:gz") as tar:
            for path, data in CONTENTS.items():
                member = tarfile.TarInfo("demo-1.0.0/" + path)
                member.size = len(data)
                tar.addfile(member, io.BytesIO(data))
        response = SimpleNamespace(returncode=0, stdout=json.dumps({"packages": [package()]}).encode())
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(NS["subprocess"], "run", return_value=response) as run:
            metadata, proofs = NS["_probe_packaged_metadata"](archive.getvalue(), "demo", "1.0.0",
                                                       directory, {"PATH": "/toolchain"})
            args, kwargs = run.call_args
            self.assertEqual(args[0][:6], ["cargo", "metadata", "--locked", "--offline",
                                          "--no-deps", "--format-version"])
            self.assertEqual(kwargs["env"], {"PATH": "/toolchain"})
            self.assertEqual(metadata["readme"], "Exact README\n")
            self.assertEqual(proofs[0]["raw_requirement"], "2.0.0")
            self.assertEqual(proofs[0]["canonical_requirement"], "^2.0.0")


if __name__ == "__main__":
    unittest.main()
