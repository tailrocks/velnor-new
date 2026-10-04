"""Real OriginalFS to cold-SDK invalidation bridge; qualification is synthetic only."""
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from types import MappingProxyType
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_context as context_owner
import source_intent_cold_install as installer
import source_intent_cold_manifest as manifest_owner
import source_intent_cold_recipe as recipe_owner
import source_intent_cold_sdk as sdk_owner
from source_intent_cold_common import ColdSourceIntent


LEAVES = (
    "mise", "cargo", "rustup-home", "rustup-bootstrap", "mise-config",
    "mise-system-config", "trusted-bin",
)
TOOLCHAIN = "1.98.1-x86_64-unknown-linux-gnu"


def _witness(root):
    recipe = {"purpose": "source-intent-cold-sdk", "host": "x86_64-unknown-linux-gnu",
              "installer_source_identity": "a" * 64,
              "mise_qualification_sha256": "b" * 64}
    phases = (
        {"name": "clear", "source_sha256": "c" * 64, "exit_code": 0,
         "wall_ns": 1, "stdout_sha256": "d" * 64},
        {"name": "acquire", "source_sha256": "e" * 64, "exit_code": 0,
         "wall_ns": 2, "stdout_sha256": "f" * 64},
        {"name": "install", "source_sha256": "g" * 64, "exit_code": 0,
         "wall_ns": 3, "stdout_sha256": "h" * 64},
    )
    witness = object.__new__(installer.FreshColdInstallationWitness)
    values = {"_recipe": MappingProxyType(recipe), "_root": str(root),
              "_phases": tuple(MappingProxyType(item) for item in phases),
              "_environment": MappingProxyType({}),
              "_seal": installer._INSTALLATION_SEAL}
    for name, value in values.items():
        object.__setattr__(witness, name, value)
    return witness


def _profile():
    return {
        "schema": 1,
        "purpose": "source-intent-cold-sdk-foundation",
        "host": "x86_64-unknown-linux-gnu",
        "runtime_abi": "source-original-fs-v3",
        "qualification_sha256": "c" * 64,
        "installer_source_identity": "a" * 64,
        "mise_qualification_sha256": "b" * 64,
    }


def _layout(root):
    for leaf in LEAVES:
        (root / leaf).mkdir()
    tool_root = root / "rustup-home" / "toolchains" / TOOLCHAIN / "bin"
    tool_root.mkdir(parents=True)
    for tool in ("cargo", "rustc", "rustdoc"):
        path = tool_root / tool
        path.write_bytes(b"#!/bin/sh\nprintf cold-tool\n")
        path.chmod(0o755)


def _sdk_from_real_inventory(root):
    # Synthetic seams are limited to Foundation profile, witness freshness, and
    # SDK qualification. OriginalFS inventory and executable hashes stay real.
    witness = _witness(root)
    context = context_owner.FreshColdInstallationContext(
        witness, _seal=context_owner._CONTEXT_SEAL)
    try:
        result = manifest_owner.source_original_inventory(context)
        payload = json.loads(result.canonical_bytes)
        tools = {}
        for tool in ("cargo", "rustc", "rustdoc"):
            relative = "rustup-home/toolchains/" + TOOLCHAIN + "/bin/" + tool
            entry = next(item for item in payload["entries"] if item["path"] == relative)
            tools[tool] = (str(root / relative), entry["sha256"])
        recipe = dict(witness._recipe)
        genesis = {"schema": 1, "purpose": recipe["purpose"],
                   "phases": [dict(item) for item in witness._phases],
                   "host": recipe["host"],
                   "installer_source_identity": recipe["installer_source_identity"],
                   "mise_qualification_sha256": recipe["mise_qualification_sha256"],
                   "foundation_qualification_sha256": _profile()["qualification_sha256"],
                   "installed_manifest_sha256": result.digest,
                   "tool_identities": {
                       name: name + " fixture" for name in recipe_owner._TOOL_IDENTITY_NAMES
                   },
                   "installed_obligations": {
                       name: name + " fixture"
                       for name in recipe_owner._INSTALLED_OBLIGATION_NAMES
                   },
                   "recipe_sha256": recipe_owner._record_digest(recipe)}
        with patch.object(sdk_owner, "_require_qualification"):
            sdk = sdk_owner.ColdSourceIntentSdk(
                context, result.canonical_bytes, tools, genesis, _seal=sdk_owner._SDK_SEAL)
        return context, sdk
    except BaseException:
        context.close()
        raise


class ColdCurrentBridgeTests(unittest.TestCase):
    def test_real_original_inventory_invalidates_sdk_and_tool_on_metadata_drift(self):
        mutations = ("mode", "mtime", "inode")
        with patch.object(context_owner, "_COMPILED_COLD_FOUNDATION_PROFILE", _profile()), \
                patch.object(installer.FreshColdInstallationWitness, "require_current"):
            for mutation in mutations:
                with self.subTest(mutation=mutation), \
                        tempfile.TemporaryDirectory(prefix="velnor-cold-current-") as directory:
                    root = Path(os.path.realpath(directory)) / "root"
                    root.mkdir()
                    _layout(root)
                    context, sdk = _sdk_from_real_inventory(root)
                    try:
                        proof = sdk.installed_tool("rustc")
                        rustc = root / "rustup-home" / "toolchains" / TOOLCHAIN / "bin" / "rustc"
                        original = rustc.read_bytes()
                        before = rustc.stat()
                        if mutation == "mode":
                            rustc.chmod(0o754)
                        elif mutation == "mtime":
                            info = rustc.stat()
                            os.utime(rustc, ns=(info.st_atime_ns, info.st_mtime_ns + 1_000_000_000))
                        else:
                            info = rustc.stat()
                            replacement = rustc.with_name("rustc-replacement")
                            replacement.write_bytes(original)
                            replacement.chmod(info.st_mode & 0o7777)
                            os.utime(replacement, ns=(info.st_atime_ns, info.st_mtime_ns))
                            os.replace(replacement, rustc)
                            after = rustc.stat()
                            self.assertNotEqual(after.st_ino, before.st_ino)
                            self.assertEqual(rustc.read_bytes(), original)
                            self.assertEqual(after.st_mode, before.st_mode)
                            self.assertEqual(after.st_mtime_ns, before.st_mtime_ns)
                        actions = (
                            sdk.require_current, proof.require_current,
                            lambda: proof.path, lambda: proof.sha256,
                            sdk.genesis, lambda: sdk.installed_tool("cargo"),
                            lambda: sdk.rust_version, lambda: sdk.host,
                            lambda: sdk.require_policy("1.98.1", "x86_64-unknown-linux-gnu"),
                        )
                        for action in actions:
                            with self.subTest(mutation=mutation, action=action), \
                                    self.assertRaisesRegex(ColdSourceIntent,
                                                           "cold_sdk_postgrant_mutation"):
                                action()
                    finally:
                        context.close()
                    with self.assertRaisesRegex(ColdSourceIntent,
                                                 "cold_sdk_foundation_context_authority"):
                        context.require_current()

    def test_real_inventory_contains_owner_and_opaque_metadata_records(self):
        with patch.object(context_owner, "_COMPILED_COLD_FOUNDATION_PROFILE", _profile()), \
                patch.object(installer.FreshColdInstallationWitness, "require_current"):
            with tempfile.TemporaryDirectory(prefix="velnor-cold-current-") as directory:
                root = Path(os.path.realpath(directory)) / "root"
                root.mkdir()
                _layout(root)
                context = context_owner.FreshColdInstallationContext(
                    _witness(root), _seal=context_owner._CONTEXT_SEAL)
                try:
                    result = manifest_owner.source_original_inventory(context)
                    value = json.loads(result.canonical_bytes)
                    self.assertEqual(value["schema"], 3)
                    self.assertIn("owner", value)
                    self.assertTrue(all("metadata" in entry and "local" in entry
                                        for entry in value["entries"]))
                finally:
                    context.close()


if __name__ == "__main__":
    unittest.main()
