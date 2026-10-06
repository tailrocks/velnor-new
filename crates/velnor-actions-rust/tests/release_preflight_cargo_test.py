"""Controlled Cargo package fixtures for the fixed Rust release source owner."""
import os
import hashlib
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch


class ReconcileError(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise ReconcileError(reason)


ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "cargo_preflight_test", "require": require}
exec(compile((ROOT / "release_source_validation.py").read_text(),
             "release_source_validation.py", "exec"), NS)
exec(compile((ROOT / "release_preflight_cargo.py").read_text(),
             "release_preflight_cargo.py", "exec"), NS)
APPROVED = {"source_sha": "a" * 40, "packages": {"second": "2.0.0", "demo": "1.0.0"},
            "tools": {"rust": "1.98.1"}}


class CargoPreflightTests(unittest.TestCase):
    def test_fixed_packager_scrubs_authority_and_returns_selected_inventory(self):
        calls = []

        def run(argv, **kwargs):
            calls.append((argv, kwargs))
            destination = Path(kwargs["cwd"]) / "package"
            destination.mkdir()
            for name, version in APPROVED["packages"].items():
                (destination / f"{name}-{version}.crate").write_bytes(name.encode())
            return SimpleNamespace(returncode=0)

        def inventory(data, name, version, source_sha):
            self.assertEqual(data, name.encode())
            self.assertEqual(source_sha, APPROVED["source_sha"])
            return {"version": version}

        def metadata(data, name, version, directory, environment):
            # Persist the inventoried bytes even if the temporary archive changes later.
            (Path(directory) / "package" / f"{name}-{version}.crate").write_bytes(b"changed")
            return {"deps": []}, []

        environment = {"RELEASE_RUST_TOOLCHAIN": "1.98.1", "GH_TOKEN": "secret",
                       "GITHUB_ENV": "/tmp/poison", "CARGO_HOME": "/tmp/poison"}
        with tempfile.TemporaryDirectory() as directory, \
             patch.dict(os.environ, environment), \
             patch.dict(NS, {"inventory": inventory,
                             "_probe_packaged_metadata": metadata,
                             "selected_publication_order": lambda packages: sorted(packages)}), \
             patch.object(NS["subprocess"], "run", side_effect=run):
            destination = Path(directory) / "release-package"
            packages = NS["approved_package_inventory"](
                APPROVED, Path("/approved-source"), Path("Cargo.toml"), destination)
            for name, version in APPROVED["packages"].items():
                self.assertEqual((destination / "crates" / f"{name}-{version}.crate").read_bytes(),
                                 name.encode())
                self.assertEqual(packages[name]["version"], version)
                self.assertEqual(packages[name]["dependencies"], [])
                self.assertEqual(packages[name]["archive_sha256"], hashlib.sha256(name.encode()).hexdigest())
        argv, kwargs = calls[0]
        self.assertEqual(argv[:6], ["cargo", "package", "--locked",
                                   "--manifest-path", "/approved-source/Cargo.toml", "--target-dir"])
        self.assertEqual(argv[-4:], ["--package", "demo", "--package", "second"])
        self.assertNotIn("GH_TOKEN", kwargs["env"])
        self.assertNotIn("GITHUB_ENV", kwargs["env"])
        self.assertEqual(kwargs["env"]["RUSTUP_TOOLCHAIN"], "1.98.1")
        self.assertEqual(kwargs["env"]["CARGO_HOME"], str(Path(kwargs["cwd"]) / "cargo-home"))
        self.assertNotEqual(kwargs["cwd"], "/approved-source")

    def test_failed_packager_cannot_create_approved_evidence(self):
        with patch.dict(os.environ, {"RELEASE_RUST_TOOLCHAIN": "1.98.1"}), \
             patch.object(NS["subprocess"], "run", return_value=SimpleNamespace(returncode=1)):
            with self.assertRaisesRegex(ReconcileError, "approved_cargo_package_failed"):
                NS["approved_package_inventory"](APPROVED, Path("/source"), Path("Cargo.toml"))

    def test_success_without_selected_archives_fails_closed(self):
        with patch.dict(os.environ, {"RELEASE_RUST_TOOLCHAIN": "1.98.1"}), \
             patch.object(NS["subprocess"], "run", return_value=SimpleNamespace(returncode=0)):
            with self.assertRaisesRegex(ReconcileError, "approved_archive_missing"):
                NS["approved_package_inventory"](APPROVED, Path("/source"), Path("Cargo.toml"))


if __name__ == "__main__":
    unittest.main()
