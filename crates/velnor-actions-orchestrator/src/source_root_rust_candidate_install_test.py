"""Root Rust candidate install and observation gates; fixtures grant no SDK."""
import copy
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_context as cold_context
import source_intent_cold_foundation as foundation_owner
import source_intent_cold_sdk as cold_sdk
import source_root_rust_candidate_context as candidate_context
import source_root_rust_candidate_install as candidate_install
import source_root_rust_candidate_recipe as candidate_recipe
import source_root_rust_candidate_recipe_test as recipe_fixture
from source_intent_cold_common import ColdSourceIntent


def _recipe():
    return copy.deepcopy(recipe_fixture._recipe())


class _SyntheticCandidateOwner:
    """Test issuer only; its fields are not native qualification evidence."""

    compiled_role = "RootRustCandidate"

    def __init__(self, root, events, ca_file):
        self.candidate_root = root
        self.ca_file = ca_file
        self.events = events
        self.native_failure = None
        self.compiler_failure = None
        self.installed_failure = None

    def require_current(self):
        self.events.append(("foundation", self.candidate_root))

    def require_acquired_native(self):
        self.events.append(("native", self.candidate_root))
        if self.native_failure is not None:
            raise self.native_failure

    def require_compiler_source_native(self):
        self.events.append(("compiler", self.candidate_root))
        if self.compiler_failure is not None:
            raise self.compiler_failure

    def require_installed_native(self):
        self.events.append(("installed", self.candidate_root))
        if self.installed_failure is not None:
            raise self.installed_failure


def _foundation_fixture(root, events, ca_file="/etc/ssl/cert.pem"):
    owner = _SyntheticCandidateOwner(root, events, ca_file)
    getter = lambda: (events.append(("getter", root)), owner)[1]
    binding = patch.multiple(
        foundation_owner,
        _COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_TYPE=type(owner),
        _COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_GETTER=getter,
    )
    return owner, binding


def _acquired_layout(root):
    for leaf in ("cargo-home", "rustup-home", "native-dist", "rustup-bootstrap"):
        (root / leaf).mkdir()
    (root / "cargo-home" / "bin").mkdir()
    raw_manager = root / "rustup-bootstrap" / "rustup-init"
    raw_manager.write_bytes(b"raw-rustup-init-fixture")
    raw_manager.chmod(0o755)


def _installed_layout(root):
    (root / "manager-bin").mkdir()
    manager = root / "cargo-home" / "bin" / "rustup"
    manager.write_bytes(b"candidate-manager-fixture")
    manager.chmod(0o755)
    os.symlink(str(manager), root / "manager-bin" / "rustup")


def _fixture_stage(root, recipe, events):
    def run_stage(stage, _environment):
        events.append(("stage", stage["name"]))
        if stage["name"] == "acquire":
            _acquired_layout(root)
        elif stage["name"] == "install":
            _installed_layout(root)
        return {"name": stage["name"], "source_sha256": stage["source_sha256"],
                "exit_code": 0, "wall_ns": 1, "stdout_sha256": "a" * 64}
    return run_stage


class CandidateInstallTests(unittest.TestCase):
    def test_missing_foundation_denies_before_any_stage(self):
        events = []
        with patch.object(candidate_install, "_compiled_candidate_recipe", return_value=_recipe()), \
                patch.object(foundation_owner, "_COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_TYPE", None), \
                patch.object(foundation_owner, "_COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_GETTER", None), \
                patch.object(candidate_install, "_run_stage", side_effect=lambda stage, _env:
                             events.append(stage["name"])), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_foundation_issuer_unavailable"):
            candidate_install.execute_root_rust_candidate()
        self.assertEqual(events, [])

    def test_foundation_keeps_exact_candidate_issuer_and_calls_getter_once(self):
        root = "/fixture/root"
        events = []
        owner, binding = _foundation_fixture(root, events)
        with binding:
            foundation = foundation_owner.require_preparation_foundation(
                "root-linux-compiler-candidate")
            self.assertIs(foundation._owner, owner)
            self.assertEqual(owner.compiled_role, "RootRustCandidate")
            self.assertEqual(owner.candidate_root, root)
            foundation.require_candidate_root(root)
        self.assertEqual([item for item in events if item[0] == "getter"],
                         [("getter", root)])

    def test_loader_proof_is_private_zero_argument(self):
        owner, binding = _foundation_fixture("/fixture/root", [])
        with binding:
            foundation = foundation_owner.require_preparation_foundation(
                "root-linux-compiler-candidate")
            proof = Mock()
            owner.require_acquired_native = proof
            foundation.require_acquired_native()
        proof.assert_called_once_with()

    def test_wrong_candidate_root_denies_before_any_stage(self):
        recipe = _recipe()
        root = "/fixture/root"
        events = []
        owner, binding = _foundation_fixture(root + "/wrong", events)
        with binding, \
                patch.object(candidate_install, "_compiled_candidate_recipe", return_value=recipe), \
                patch.object(candidate_install, "_candidate_environment",
                             return_value=(root, {})), \
                patch.object(candidate_install, "_run_stage", side_effect=lambda stage, _env:
                             events.append(("stage", stage["name"]))), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_foundation_candidate_root"):
            candidate_install.execute_root_rust_candidate()
        self.assertEqual([event for event in events if event[0] == "stage"], [])
        self.assertEqual(owner.candidate_root, root + "/wrong")

    def test_wrong_manager_digest_after_acquire_stops_before_install(self):
        recipe = _recipe()
        events = []
        owner, binding = _foundation_fixture("/fixture", events)

        def run_stage(stage, _environment):
            events.append(("stage", stage["name"]))
            return {"name": stage["name"], "source_sha256": stage["source_sha256"],
                    "exit_code": 0, "wall_ns": 1, "stdout_sha256": "a" * 64}

        with binding, \
                patch.object(candidate_install, "_compiled_candidate_recipe", return_value=recipe), \
                patch.object(candidate_install, "_candidate_environment",
                             return_value=("/fixture", {})), \
                patch.object(candidate_install, "_run_stage", side_effect=run_stage), \
                patch.object(candidate_install, "_require_empty_candidate"), \
                patch.object(candidate_install, "_regular_hash", return_value="wrong"), \
                self.assertRaisesRegex(ColdSourceIntent,
                                       "root_candidate_acquired_manager_changed"):
            candidate_install.execute_root_rust_candidate()
        self.assertEqual([event[1] for event in events if event[0] == "stage"],
                         ["clear", "acquire"])
        self.assertNotIn("native", [event[0] for event in events])

    def test_missing_native_methods_stop_before_relevant_stage(self):
        recipe = _recipe()
        cases = (
            ("require_acquired_native", "cold_foundation_acquired_native_unavailable",
             ["clear", "acquire"]),
            ("require_compiler_source_native", "cold_foundation_compiler_source_native_unavailable",
             ["clear", "acquire"]),
            ("require_installed_native", "cold_foundation_installed_native_unavailable",
             ["clear", "acquire", "install"]),
        )
        for method, reason, expected in cases:
            with self.subTest(method=method), \
                    tempfile.TemporaryDirectory(prefix="root-rust-candidate-proof-") as directory:
                root = Path(os.path.realpath(directory)) / recipe["namespace"]
                root.mkdir(parents=True)
                events = []
                owner, binding = _foundation_fixture(str(root), events)
                with binding, \
                        patch.object(owner, method, None), \
                        patch.object(candidate_install, "_compiled_candidate_recipe",
                                     return_value=recipe), \
                        patch.object(candidate_install, "_candidate_environment",
                                     return_value=(str(root), {})), \
                        patch.object(candidate_install, "_run_stage",
                                     side_effect=_fixture_stage(root, recipe, events)), \
                        patch.object(candidate_install, "_regular_hash",
                                     return_value=recipe["manager_sha256"]), \
                        self.assertRaisesRegex(ColdSourceIntent, reason):
                    candidate_install.execute_root_rust_candidate()
                self.assertEqual([event[1] for event in events if event[0] == "stage"],
                                 expected)

    def test_validator_failure_stops_before_native_install(self):
        recipe = _recipe()
        for attribute, reason in (
                ("native_failure", "fixture_native_loader_failed"),
                ("compiler_failure", "fixture_compiler_source_failed")):
            with self.subTest(attribute=attribute), \
                    tempfile.TemporaryDirectory(prefix="root-rust-candidate-proof-") as directory:
                root = Path(os.path.realpath(directory)) / recipe["namespace"]
                root.mkdir(parents=True)
                events = []
                owner, binding = _foundation_fixture(str(root), events)
                setattr(owner, attribute, ColdSourceIntent(reason))
                with binding, \
                        patch.object(candidate_install, "_compiled_candidate_recipe",
                                     return_value=recipe), \
                        patch.object(candidate_install, "_candidate_environment",
                                     return_value=(str(root), {})), \
                        patch.object(candidate_install, "_run_stage",
                                     side_effect=_fixture_stage(root, recipe, events)), \
                        patch.object(candidate_install, "_regular_hash",
                                     return_value=recipe["manager_sha256"]), \
                        self.assertRaisesRegex(ColdSourceIntent, reason):
                    candidate_install.execute_root_rust_candidate()
                self.assertEqual([event[1] for event in events if event[0] == "stage"],
                                 ["clear", "acquire"])

    def _witness_fixture(self, root="/fixture"):
        recipe = _recipe()
        environment = {"RUNNER_TEMP": "/fixture-temp"}
        events = []
        owner, binding = _foundation_fixture(root, events)
        with binding:
            foundation = foundation_owner.require_preparation_foundation(
                "root-linux-compiler-candidate")
            with patch.object(candidate_install, "_compiled_candidate_recipe",
                              return_value=recipe), \
                    patch.object(candidate_install, "_candidate_environment",
                                 return_value=(root, environment)), \
                    patch.object(candidate_install, "_regular_hash",
                                 return_value=recipe["manager_sha256"]):
                witness = candidate_install.RootRustCandidateInstallationWitness(
                    recipe, root, environment, [], foundation,
                    _seal=candidate_install._CANDIDATE_INSTALLATION_SEAL)
        return recipe, witness, owner, binding

    def test_witness_is_immutable_and_rejects_source_or_environment_change(self):
        recipe, witness, _owner, binding = self._witness_fixture()
        with self.assertRaisesRegex(ColdSourceIntent, "root_candidate_witness_immutable"):
            witness._root = "/foreign"
        changed = copy.deepcopy(recipe)
        changed["stages"][0]["source"] += "\n"
        with binding, \
                patch.object(candidate_install, "_compiled_candidate_recipe",
                             return_value=changed), \
                patch.object(candidate_install, "_candidate_environment",
                             return_value=(witness._root, dict(witness._environment))), \
                patch.object(candidate_install, "_regular_hash",
                             return_value=recipe["manager_sha256"]), \
                self.assertRaisesRegex(ColdSourceIntent, "root_candidate_source_changed"):
            witness.require_current()

    def test_witness_current_rechecks_acquired_and_installed_proofs(self):
        recipe, witness, owner, binding = self._witness_fixture()
        before = {name: sum(event[0] == name for event in owner.events)
                  for name in ("native", "installed")}
        with binding, \
                patch.object(candidate_install, "_compiled_candidate_recipe",
                             return_value=recipe), \
                patch.object(candidate_install, "_candidate_environment",
                             return_value=(witness._root, dict(witness._environment))), \
                patch.object(candidate_install, "_regular_hash",
                             return_value=recipe["manager_sha256"]):
            witness.require_current()
        self.assertEqual(sum(event[0] == "native" for event in owner.events),
                         before["native"] + 1)
        self.assertEqual(sum(event[0] == "installed" for event in owner.events),
                         before["installed"] + 1)
        with binding, \
                patch.object(candidate_install, "_compiled_candidate_recipe",
                             return_value=recipe), \
                patch.object(candidate_install, "_candidate_environment",
                             return_value=("/foreign", dict(witness._environment))), \
                self.assertRaisesRegex(ColdSourceIntent, "root_candidate_source_changed"):
            witness.require_current()

    def test_copied_foundation_or_witness_capability_is_denied(self):
        _recipe_value, witness, _owner, binding = self._witness_fixture()
        with binding:
            with self.assertRaisesRegex(ColdSourceIntent, "cold_foundation_immutable"):
                copy.copy(witness._foundation)
        with self.assertRaisesRegex(ColdSourceIntent, "root_candidate_witness_immutable"):
            copy.copy(witness)

    def test_candidate_cannot_enter_cold_context_or_mint_sdk(self):
        with tempfile.TemporaryDirectory(prefix="root-rust-candidate-context-") as directory:
            root = Path(os.path.realpath(directory)) / "root"
            root.mkdir(parents=True); [ (root / leaf).mkdir() for leaf in candidate_recipe._LEAVES]
            recipe, witness, _owner, binding = self._witness_fixture(str(root))
            with binding, \
                    patch.object(candidate_install, "_compiled_candidate_recipe",
                                 return_value=recipe), \
                    patch.object(candidate_install, "_candidate_environment",
                                 return_value=(str(root), dict(witness._environment))), \
                    patch.object(candidate_install, "_regular_hash",
                                 return_value=recipe["manager_sha256"]):
                context = candidate_context.FreshRootRustCandidateContext(
                    witness, _seal=candidate_context._CONTEXT_SEAL)
                try:
                    with self.assertRaisesRegex(ColdSourceIntent,
                                                "cold_sdk_foundation_context_authority"):
                        cold_context._from_completed_installation(witness)
                    with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_authority"):
                        cold_sdk.ColdSourceIntentSdk(
                            context, b"fixture", {}, {}, _seal=cold_sdk._SDK_SEAL)
                finally:
                    context.close()

if __name__ == "__main__":
    unittest.main()
