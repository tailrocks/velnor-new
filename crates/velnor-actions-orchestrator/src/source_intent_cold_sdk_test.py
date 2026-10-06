"""Cold SDK attenuation tests; fixture seals never qualify a production install."""
import hashlib
import sys
import json
import unittest
from pathlib import Path
from types import MappingProxyType, SimpleNamespace
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


_RECIPE = {
    "purpose": "source-intent-cold-sdk",
    "host": "x86_64-unknown-linux-gnu",
    "installer_source_identity": "s" * 64,
    "mise_qualification_sha256": "q" * 64,
}
_PHASES = (
    {"name": "clear", "source_sha256": "a" * 64, "exit_code": 0,
     "wall_ns": 1, "stdout_sha256": "b" * 64},
    {"name": "acquire", "source_sha256": "c" * 64, "exit_code": 0,
     "wall_ns": 2, "stdout_sha256": "d" * 64},
    {"name": "install", "source_sha256": "e" * 64, "exit_code": 0,
     "wall_ns": 3, "stdout_sha256": "f" * 64},
)
_FOUNDATION_QUALIFICATION = "g" * 64


def _context():
    witness = SimpleNamespace(
        _recipe=MappingProxyType(dict(_RECIPE)), root="/fixture",
        _phases=tuple(MappingProxyType(dict(item)) for item in _PHASES))
    context = object.__new__(context_owner.FreshColdInstallationContext)
    object.__setattr__(context, "_witness", witness)
    object.__setattr__(context, "_descriptor", -1)
    object.__setattr__(context, "_identity", ())
    object.__setattr__(context, "_profile", MappingProxyType(
        {"qualification_sha256": _FOUNDATION_QUALIFICATION}))
    object.__setattr__(context, "_seal", context_owner._CONTEXT_SEAL)
    return context


def _tools():
    return {tool: ("/fixture/" + tool, "c" * 64)
            for tool in ("cargo", "rustc", "rustdoc")}


def _genesis(context=None, manifest=b"fixture-manifest"):
    context = context or _context()
    recipe = dict(context._witness._recipe)
    return {
        "schema": 1,
        "purpose": recipe["purpose"],
        "phases": [dict(item) for item in context._witness._phases],
        "host": recipe["host"],
        "installer_source_identity": recipe["installer_source_identity"],
        "mise_qualification_sha256": recipe["mise_qualification_sha256"],
        "foundation_qualification_sha256": _FOUNDATION_QUALIFICATION,
        "installed_manifest_sha256": hashlib.sha256(manifest).hexdigest(),
        "recipe_sha256": recipe_owner._record_digest(recipe),
        "tool_identities": {name: name + " fixture identity"
                            for name in recipe_owner._TOOL_IDENTITY_NAMES},
        "installed_obligations": {name: name + " output" for name in recipe_owner._INSTALLED_OBLIGATION_NAMES},
    }


def _tool_manifest():
    entries, tools = [], {}
    root = "/fixture"
    for tool in ("cargo", "rustc", "rustdoc"):
        relative = "rustup-home/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/" + tool
        digest = tool[0] * 64
        entries.append({"path": relative, "kind": "file", "sha256": digest})
        tools[tool] = (root + "/" + relative, digest)
    manifest = json.dumps({"schema": 3, "entries": entries}, separators=(",", ":")).encode()
    return manifest, tools


class ColdSdkTests(unittest.TestCase):
    def setUp(self):
        self.context_current = patch.object(
            context_owner.FreshColdInstallationContext, "require_current", return_value=None)
        self.context_current.start()
        self.inventory = patch.object(
            manifest_owner, "source_original_inventory",
            return_value=SimpleNamespace(canonical_bytes=b"fixture-manifest"))
        self.inventory.start()
        self.regular_hash = patch.object(sdk_owner, "_regular_hash", return_value="c" * 64)
        self.regular_hash.start()
        self.addCleanup(self.regular_hash.stop)
        self.addCleanup(self.inventory.stop)
        self.addCleanup(self.context_current.stop)
        self.sdk = self._make_sdk()

    def _make_sdk(self, context=None):
        context = context or _context()
        tools = _tools()
        with patch.object(sdk_owner, "_require_qualification"), \
                patch.object(sdk_owner, "_require_tool_bindings"), \
                patch.object(sdk_owner.ColdSourceIntentSdk, "require_current"):
            return sdk_owner.ColdSourceIntentSdk(
                context, b"fixture-manifest", tools, _genesis(context),
                _seal=sdk_owner._SDK_SEAL)

    def test_private_seals_reject_forged_context_sdk_and_tool(self):
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_authority"):
            sdk_owner.ColdSourceIntentSdk(_context(), b"manifest", {}, {}, _seal=object())
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_authority"):
            sdk_owner.ColdSourceIntentSdk(object(), b"manifest", {}, {}, _seal=sdk_owner._SDK_SEAL)
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_tool_authority"):
            sdk_owner.ColdSourceIntentInstalledTool(self.sdk, "cargo", _seal=object())
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_tool_authority"):
            sdk_owner.ColdSourceIntentInstalledTool(object(), "cargo", _seal=sdk_owner._TOOL_SEAL)
        with self.assertRaises(ColdSourceIntent):
            self.sdk._forged = True

    def test_missing_native_source_authority_denies_matching_version_observation(self):
        observations = {
            "foundation_qualification_sha256": "a" * 64,
            "tool_identities": {name: name + " fixture identity"
                                 for name in recipe_owner._TOOL_IDENTITY_NAMES},
            "installed_obligations": {
                name: name + " output" for name in recipe_owner._INSTALLED_OBLIGATION_NAMES
            },
        }
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_native_installation_authority_unavailable"):
            recipe_owner._require_qualification({"compiler_source_authority": None}, observations)

    def test_genesis_binds_actual_witness_metadata_and_manifest(self):
        mutations = {
            "schema": 2,
            "purpose": "foreign-purpose",
            "phases": [],
            "host": "aarch64-unknown-linux-gnu",
            "installer_source_identity": "x" * 64,
            "mise_qualification_sha256": "y" * 64,
            "foundation_qualification_sha256": "z" * 64,
            "installed_manifest_sha256": "0" * 64,
            "recipe_sha256": "1" * 64,
        }
        for field, value in mutations.items():
            with self.subTest(field=field):
                context = _context()
                genesis = _genesis(context)
                genesis[field] = value
                with patch.object(sdk_owner, "_require_qualification"), \
                        patch.object(sdk_owner, "_require_tool_bindings"), \
                        patch.object(sdk_owner.ColdSourceIntentSdk, "require_current"), \
                        self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_genesis_binding"):
                    sdk_owner.ColdSourceIntentSdk(
                        context, b"fixture-manifest", _tools(), genesis,
                        _seal=sdk_owner._SDK_SEAL)

        context = _context()
        genesis = _genesis(context)
        genesis["unexpected"] = True
        with patch.object(sdk_owner, "_require_qualification"), \
                patch.object(sdk_owner, "_require_tool_bindings"), \
                patch.object(sdk_owner.ColdSourceIntentSdk, "require_current"), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_genesis_binding"):
            sdk_owner.ColdSourceIntentSdk(
                context, b"fixture-manifest", _tools(), genesis,
                _seal=sdk_owner._SDK_SEAL)

    def test_fixture_source_capsule_ignores_mutated_run_manifest(self):
        recipe = {"compiler_source_authority": {"role": "owned-cargo-verifier"},
                  "host": "x86_64-unknown-linux-gnu",
                  "installer_source_identity": "b" * 64,
                  "mise_qualification_sha256": "c" * 64,
                  "expected_obligations": json.loads(json.dumps(recipe_owner._EXPECTED_OBLIGATIONS))}
        observations = {"foundation_qualification_sha256": "a" * 64,
                        "tool_identities": {
                            name: name + " fixture identity"
                            for name in recipe_owner._TOOL_IDENTITY_NAMES
                        },
                        "installed_obligations": {
                            name: name + " output"
                            for name in recipe_owner._INSTALLED_OBLIGATION_NAMES
                        },
                        "installed_manifest_sha256": "d" * 64}
        expected = {
            "schema": 1,
            "purpose": "source-intent-cold-sdk",
            "recipe_sha256": recipe_owner._record_digest(recipe),
            "host": recipe["host"],
            "installer_source_identity": recipe["installer_source_identity"],
            "mise_qualification_sha256": recipe["mise_qualification_sha256"],
            "compiler_source_authority": recipe["compiler_source_authority"],
            "foundation_qualification_sha256": observations["foundation_qualification_sha256"],
            "tool_identities": observations["tool_identities"],
            "installed_obligations": observations["installed_obligations"],
            "expected_obligations": recipe["expected_obligations"],
        }
        with patch.object(recipe_owner, "_COMPILED_COLD_SDK_QUALIFICATION", expected), \
                patch.object(recipe_owner, "require_compiler_projection"):
            recipe_owner._require_qualification(recipe, observations)
            observations["installed_manifest_sha256"] = "e" * 64
            recipe_owner._require_qualification(recipe, observations)

    def test_policy_requires_exact_version_and_actual_host(self):
        with patch.object(sdk_owner.ColdSourceIntentSdk, "require_current"):
            for version, host in (("1.99.0", "x86_64-unknown-linux-gnu"),
                                  ("1.98.1", "aarch64-apple-darwin")):
                with self.subTest(version=version, host=host), \
                        self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_policy"):
                    self.sdk.require_policy(version, host)

    def test_every_spawn_rechecks_full_manifest_after_mutation(self):
        proof = self.sdk.installed_tool("cargo")
        self.inventory.stop()
        changed = patch.object(
            manifest_owner, "source_original_inventory",
            return_value=SimpleNamespace(canonical_bytes=b"mutated-manifest"))
        changed.start()
        self.addCleanup(changed.stop)
        actions = (self.sdk.require_current, proof.require_current,
                   lambda: proof.path, self.sdk.genesis,
                   lambda: self.sdk.installed_tool("rustdoc"))
        for action in actions:
            with self.subTest(action=action), \
                    self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_postgrant_mutation"):
                action()

    def test_tool_proof_is_immutable_and_exposes_only_path_digest(self):
        proof = self.sdk.installed_tool("rustdoc")
        self.assertEqual(proof.path, "/fixture/rustdoc")
        self.assertEqual(proof.sha256, "c" * 64)
        with self.assertRaises(ColdSourceIntent):
            proof._tool = "cargo"
        self.assertNotIn("metadata", self.sdk.genesis())

    def test_tool_binding_rejects_missing_keys_and_foreign_paths(self):
        manifest, tools = _tool_manifest()
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_tool_inventory"):
            sdk_owner._require_tool_bindings(_context(), manifest, {"cargo": tools["cargo"]})
        foreign = dict(tools, rustc=("/foreign/rustc", tools["rustc"][1]))
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_tool_binding"):
            sdk_owner._require_tool_bindings(_context(), manifest, foreign)
        wrong_digest = dict(tools, cargo=(tools["cargo"][0], "f" * 64))
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_tool_binding"):
            sdk_owner._require_tool_bindings(_context(), manifest, wrong_digest)
        entries = json.loads(manifest)
        entries["entries"].pop()
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_tool_binding"):
            sdk_owner._require_tool_bindings(
                _context(), json.dumps(entries).encode(), tools)

    def test_context_private_constructor_and_unqualified_default_factory_reject(self):
        with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_foundation_context_authority"):
            context_owner.FreshColdInstallationContext(object(), _seal=context_owner._CONTEXT_SEAL)
        witness = object.__new__(installer.FreshColdInstallationWitness)
        object.__setattr__(witness, "_recipe", MappingProxyType({
            "installer_source_identity": "s" * 64,
            "mise_qualification_sha256": "q" * 64,
        }))
        object.__setattr__(witness, "_root", "/fixture")
        object.__setattr__(witness, "_phases", ())
        object.__setattr__(witness, "_seal", installer._INSTALLATION_SEAL)
        with patch.object(installer.FreshColdInstallationWitness, "require_current"):
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_foundation_qualification_unavailable"):
                context_owner._from_completed_installation(witness)


if __name__ == "__main__":
    unittest.main()
