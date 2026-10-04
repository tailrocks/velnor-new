"""Compiler projection parser tests; fixtures do not grant production authority."""
import copy
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_compiler as compiler
from source_intent_cold_common import ColdSourceIntent


def _manifest():
    release_date = "2026-01-01"
    components = []
    for component in ("rustc", "cargo", "rust-std", "clippy-preview", "rustfmt-preview"):
        archive = component.removesuffix("-preview")
        prefix = "https://static.rust-lang.org/dist/" + release_date + "/"
        prefix += archive + "-1.98.1-x86_64-unknown-linux-gnu.tar."
        components.append({
            "component": component,
            "xz_url": prefix + "xz",
            "xz_sha256": "a" * 64,
            "gzip_url": prefix + "gz",
            "gzip_sha256": "b" * 64,
        })
    return {
        "version": "1.98.1",
        "target": "x86_64-unknown-linux-gnu",
        "manifest_url": "https://static.rust-lang.org/dist/channel-rust-1.98.1.toml",
        "manifest_sha256": "c" * 64,
        "release_date": release_date,
        "rust_source_repository": "https://github.com/rust-lang/rust",
        "rust_source_commit": "d" * 40,
        "rust_source_tree": "e" * 40,
        "cargo_source_repository": "https://github.com/rust-lang/cargo",
        "cargo_source_commit": "f" * 40,
        "cargo_source_tree": "0" * 40,
        "components": components,
    }


def _record(manifest=None):
    manifest = manifest or _manifest()
    manifest_identity = compiler._manifest_identity(manifest)
    receipt = "1" * 64
    tree = "2" * 64
    transform = "owned-cargo-verifier-v1"
    qualification = compiler._identity((
        "velnor-root-rust-compiler-artifact-v1", manifest_identity,
        receipt, tree, transform))
    return {
        "schema": 1,
        "purpose": "root-linux-compiler-artifact-v1",
        "qualification_sha256": qualification,
        "manifest": manifest,
        "installation_receipt_sha256": receipt,
        "installed_tree_sha256": tree,
        "transform_abi": transform,
    }


def _check(record):
    with patch.object(compiler, "_COMPILED_COLD_COMPILER_TRANSFORM_ABI",
                      record["transform_abi"]):
        compiler.require_compiler_projection(record)


class CompilerProjectionTests(unittest.TestCase):
    def test_valid_projection_binds_official_manifest_rows_and_source_identity(self):
        record = _record()
        _check(record)
        changed = copy.deepcopy(record)
        changed["manifest"]["rust_source_commit"] = "0" * 40
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_native_installation_identity"):
            _check(changed)

    def test_audit_only_or_missing_installation_receipt_is_not_native_authority(self):
        record = _record()
        record["purpose"] = "audit-only"
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_native_installation_authority_unavailable"):
            compiler.require_compiler_projection(record)
        record = _record()
        del record["installation_receipt_sha256"]
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_native_installation_authority_unavailable"):
            compiler.require_compiler_projection(record)

    def test_manifest_shape_target_and_component_urls_are_closed(self):
        for field, value in (("version", "1.99.0"),
                             ("target", "aarch64-unknown-linux-gnu"),
                             ("manifest_url", "https://foreign.invalid/channel.toml")):
            record = _record()
            record["manifest"][field] = value
            with self.subTest(field=field), self.assertRaises(ColdSourceIntent):
                _check(record)
        record = _record()
        record["manifest"]["components"][0]["xz_url"] += ".foreign"
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_native_component_url"):
            _check(record)

    def test_qualification_digest_rejects_changed_installed_tree_or_transform(self):
        for field in ("installation_receipt_sha256", "installed_tree_sha256", "transform_abi"):
            record = _record()
            record[field] = "3" * 64 if field != "transform_abi" else "foreign-transform"
            with self.subTest(field=field), \
                    self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_native_installation_identity"):
                _check(record)

    def test_self_consistent_foreign_transform_abi_stays_unqualified(self):
        record = _record()
        record["transform_abi"] = "foreign-transform"
        record["qualification_sha256"] = compiler._identity((
            "velnor-root-rust-compiler-artifact-v1", compiler._manifest_identity(record["manifest"]),
            record["installation_receipt_sha256"], record["installed_tree_sha256"],
            record["transform_abi"]))
        with patch.object(compiler, "_COMPILED_COLD_COMPILER_TRANSFORM_ABI",
                          "owned-cargo-verifier-v1"), \
                self.assertRaisesRegex(ColdSourceIntent,
                                       "cold_sdk_native_installation_authority_unavailable"):
            compiler.require_compiler_projection(record)


if __name__ == "__main__":
    unittest.main()
