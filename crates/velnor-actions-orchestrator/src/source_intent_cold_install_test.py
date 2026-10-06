"""Cold installer stage and ordering tests with isolated fixture seals."""
import copy
import hashlib
import json
import os
import shlex
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_install as installer
import source_intent_cold_foundation as foundation_owner
import source_intent_cold_process as process_owner
import source_intent_cold_recipe as recipe_owner
from source_intent_cold_common import ColdSourceIntent


def _stage(name, source="printf cold-stage"):
    return {
        "name": name,
        "source": source,
        "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
        "arguments": [],
    }


def _recipe():
    return {
        "schema": 1,
        "purpose": "source-intent-cold-sdk",
        "role": "root-linux",
        "host": "x86_64-unknown-linux-gnu",
        "rust_version": "1.98.1",
        "toolchain": "1.98.1-x86_64-unknown-linux-gnu",
        "namespace": "velnor-control/source-intent",
        "leaves": list(installer._LEAVES),
        "stages": [_stage(name) for name in ("clear", "acquire", "install")],
        "environment": {},
        "mise_sha256": "a" * 64,
        "manager_sha256": recipe_owner._MANAGER_SHA256,
        "installer_source_identity": "b" * 64,
        "mise_qualification_sha256": "c" * 64,
        "compiler_source_authority": {},
        "expected_obligations": json.loads(json.dumps(recipe_owner._EXPECTED_OBLIGATIONS)),
    }


class _SyntheticPreparationOwner:
    """Fixture issuer only; this object is not qualification evidence."""

    def __init__(self, events):
        self.events = events

    def require_current(self):
        self.events.append("foundation-current")


def _foundation_fixture(events):
    owner = _SyntheticPreparationOwner(events)
    getter = lambda: (events.append("foundation-getter"), owner)[1]
    binding = patch.multiple(
        foundation_owner,
        _COMPILED_SOURCE_INTENT_FOUNDATION_TYPE=type(owner),
        _COMPILED_SOURCE_INTENT_FOUNDATION_GETTER=getter,
    )
    return owner, binding


class ColdInstallerTests(unittest.TestCase):
    def test_run_stage_hashes_source_and_records_actual_child(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-stage-") as directory:
            environment = {"RUNNER_TEMP": os.path.realpath(directory)}
            phase = installer._run_stage(_stage("probe"), environment)
        self.assertEqual(phase["name"], "probe")
        self.assertEqual(phase["exit_code"], 0)
        self.assertEqual(phase["stdout_sha256"], hashlib.sha256(b"cold-stage").hexdigest())
        self.assertGreaterEqual(phase["wall_ns"], 0)

    def test_run_stage_rejects_changed_source_and_failed_child(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-stage-") as directory:
            environment = {"RUNNER_TEMP": os.path.realpath(directory)}
            changed = _stage("changed")
            changed["source_sha256"] = "0" * 64
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_stage_changed"):
                installer._run_stage(changed, environment)
            failed = _stage("failed", "exit 7")
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_stage_failed:failed"):
                installer._run_stage(failed, environment)

    def test_installation_runs_clear_acquire_install_and_checks_mise_before_install(self):
        events = []
        compiled = _recipe()
        environment = {"RUNNER_TEMP": "/fixture-temp"}
        _owner, foundation = _foundation_fixture(events)

        def run_stage(stage, _environment):
            events.append("stage:" + stage["name"])
            return {"name": stage["name"], "source_sha256": stage["source_sha256"],
                    "exit_code": 0, "wall_ns": 1, "stdout_sha256": "o" * 64}

        def require_cleared(_root):
            events.append("cleared")

        def regular_hash(path):
            events.append("hash:" + path.rsplit("/", 1)[-1])
            return compiled["manager_sha256"] if path.endswith("/cargo/bin/rustup") else compiled["mise_sha256"]

        with foundation, \
                patch.object(installer, "_compiled_recipe", return_value=compiled), \
                patch.object(installer, "_runtime_environment", return_value=("/fixture", environment)), \
                patch.object(installer, "_run_stage", side_effect=run_stage), \
                patch.object(installer, "_require_cleared", side_effect=require_cleared), \
                patch.object(installer, "_regular_hash", side_effect=regular_hash):
            witness = installer.execute_cold_installation()

        phase_events = [event for event in events
                        if event.startswith(("stage:", "cleared", "hash:"))]
        self.assertEqual(phase_events[:5], ["stage:clear", "cleared", "stage:acquire",
                                            "hash:mise", "stage:install"])
        self.assertIn("hash:rustup", events)
        self.assertEqual(dict(witness._environment), environment)
        self.assertEqual(events.count("foundation-getter"), 1)
        stages = [index for index, event in enumerate(events) if event.startswith("stage:")]
        self.assertEqual([events[index - 1] for index in stages],
                         ["foundation-current"] * 3)
        with self.assertRaises(ColdSourceIntent):
            witness._environment = {}

    def test_installation_stops_on_wrong_mise_digest_before_install(self):
        events = []
        compiled = _recipe()
        _owner, foundation = _foundation_fixture(events)

        def run_stage(stage, _environment):
            events.append("stage:" + stage["name"])
            return {"name": stage["name"], "source_sha256": stage["source_sha256"],
                    "exit_code": 0, "wall_ns": 1, "stdout_sha256": "o" * 64}

        def regular_hash(path):
            events.append("hash:" + path.rsplit("/", 1)[-1])
            return "wrong" if path.endswith("/mise/bin/mise") else compiled["manager_sha256"]

        with foundation, \
                patch.object(installer, "_compiled_recipe", return_value=compiled), \
                patch.object(installer, "_runtime_environment", return_value=("/fixture", {})), \
                patch.object(installer, "_run_stage", side_effect=run_stage), \
                patch.object(installer, "_require_cleared"), \
                patch.object(installer, "_regular_hash", side_effect=regular_hash), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_acquired_mise_digest"):
            installer.execute_cold_installation()
        self.assertEqual([event for event in events
                          if event.startswith(("stage:", "hash:"))],
                         ["stage:clear", "stage:acquire", "hash:mise"])

    def test_missing_foundation_denies_before_clear_acquire_or_install(self):
        compiled = _recipe()
        events = []
        with patch.object(foundation_owner, "_COMPILED_SOURCE_INTENT_FOUNDATION_TYPE", None), \
                patch.object(foundation_owner, "_COMPILED_SOURCE_INTENT_FOUNDATION_GETTER", None), \
                patch.object(installer, "_compiled_recipe", return_value=compiled), \
                patch.object(installer, "_run_stage", side_effect=lambda stage, _env:
                             events.append("stage:" + stage["name"])), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_foundation_issuer_unavailable"):
            installer.execute_cold_installation()
        self.assertEqual(events, [])

    def test_wrong_and_candidate_purposes_deny_before_any_stage(self):
        for purpose in ("foreign-purpose", "candidate-installation-qualification"):
            with self.subTest(purpose=purpose):
                compiled = _recipe()
                compiled["purpose"] = purpose
                events = []
                _owner, foundation = _foundation_fixture(events)
                with foundation, \
                        patch.object(installer, "_compiled_recipe", return_value=compiled), \
                        patch.object(installer, "_run_stage", side_effect=lambda stage, _env:
                                     events.append("stage:" + stage["name"])), \
                        self.assertRaisesRegex(ColdSourceIntent, "cold_foundation_purpose"):
                    installer.execute_cold_installation()
                self.assertEqual(events, [])

    def test_witness_accepts_only_private_preparation_foundation(self):
        events = []
        _owner, foundation_binding = _foundation_fixture(events)
        with foundation_binding:
            foundation = foundation_owner.require_preparation_foundation(
                "source-intent-cold-sdk")
            witness_args = (_recipe(), "/fixture", [], {}, foundation)
            witness = installer.FreshColdInstallationWitness(
                *witness_args, _seal=installer._INSTALLATION_SEAL)
            self.assertIs(witness._foundation, foundation)
            for foreign in (
                    copy.copy,
                    lambda value: json.loads(json.dumps({"foundation": str(value)})),
                    lambda _value: {},
            ):
                with self.subTest(foreign=foreign):
                    def construct():
                        value = foreign(foundation)
                        installer.FreshColdInstallationWitness(
                            *witness_args[:-1], value, _seal=installer._INSTALLATION_SEAL)

                    with self.assertRaises(ColdSourceIntent):
                        construct()

    def test_runtime_environment_rejects_non_linux_actual_host(self):
        compiled = _recipe()
        with patch.object(recipe_owner.platform, "system", return_value="Darwin"), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_actual_host"):
            recipe_owner._runtime_environment(compiled)

    def test_cleared_root_rejects_unknown_or_nonempty_entries(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-root-") as directory:
            root = Path(directory).resolve()
            for leaf in installer._LEAVES:
                (root / leaf).mkdir()
            installer._require_cleared(str(root))
            (root / "foreign").write_text("foreign")
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_unowned_root_entry"):
                installer._require_cleared(str(root))
            (root / "foreign").unlink()
            (root / "cargo" / "state").write_text("state")
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_root_not_empty"):
                installer._require_cleared(str(root))

    def test_process_helper_rejects_bounded_output(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-process-") as directory:
            environment = {"PATH": "/usr/bin:/bin"}
            command = ["/bin/sh", "-c", "printf 1234567890; sleep 10"]
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_child_output_limit"):
                process_owner.observe_install_child(command, environment, directory, 4, 5)

    def test_process_helper_rejects_timeout(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-process-") as directory:
            environment = {"PATH": "/usr/bin:/bin"}
            with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_child_timeout"):
                process_owner.observe_install_child(
                    ["/bin/sh", "-c", "sleep 10"], environment, directory, 4096, 0.05)

    def test_process_helper_kills_inherited_descendant(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-process-") as directory:
            marker = os.path.join(directory, "survivor")
            script = "(sleep 0.4; echo survivor > " + shlex.quote(marker) + ") & wait"
            with self.assertRaises(ColdSourceIntent):
                process_owner.observe_install_child(
                    ["/bin/sh", "-c", script], {"PATH": "/usr/bin:/bin"}, directory,
                    4096, 0.05)
            import time
            time.sleep(0.6)
            self.assertFalse(os.path.exists(marker))

    def test_process_helper_permission_failure_is_typed_cleanup_error(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-process-") as directory, \
                patch.object(process_owner.os, "killpg",
                              side_effect=PermissionError(1, "cleanup denied")), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_child_cleanup_failed"):
            process_owner.observe_install_child(
                ["/bin/sh", "-c", "printf ok"], {"PATH": "/usr/bin:/bin"}, directory,
                4096, 5)

    def test_verified_spawn_wrong_digest_never_reaches_child_observer(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-verified-") as directory:
            directory = os.path.realpath(directory)
            path = Path(directory) / "tool"
            path.write_bytes(b"#!/bin/sh\nprintf old\n")
            path.chmod(0o755)
            with patch.object(installer, "_observe_child") as observe, \
                    self.assertRaisesRegex(ColdSourceIntent,
                                           "cold_sdk_spawn_executable_changed"):
                installer._observe_verified(str(path), "0" * 64, [], {}, directory, 4096)
            observe.assert_not_called()

    def test_verified_spawn_holds_original_fd_across_path_replacement(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-verified-") as directory:
            directory = os.path.realpath(directory)
            path = Path(directory) / "tool"
            original = b"#!/bin/sh\nprintf old\n"
            replacement = b"#!/bin/sh\nprintf replacement\n"
            path.write_bytes(original)
            path.chmod(0o755)
            expected = hashlib.sha256(original).hexdigest()
            observed = {}

            def child(command, _environment, _cwd, _limit, _timeout, *, executable_descriptor=None):
                observed["command"] = command
                observed["bytes"] = os.pread(executable_descriptor, len(original), 0)
                path.rename(path.with_suffix(".old"))
                path.write_bytes(replacement)
                path.chmod(0o755)
                return b"ok", 0, 1

            with patch.object(installer, "_observe_child", side_effect=child):
                output, status, _wall = installer._observe_verified(
                    str(path), expected, ["--probe"], {}, directory, 4096)
            self.assertEqual((output, status), (b"ok", 0))
            self.assertEqual(observed["command"], [str(path), "--probe"])
            self.assertEqual(observed["bytes"], original)
            self.assertEqual(path.read_bytes(), replacement)

    @unittest.skipUnless(sys.platform.startswith("linux"), "requires Linux /proc fd execution")
    def test_linux_elf_verified_fd_launch_preserves_argv0(self):
        with tempfile.TemporaryDirectory(prefix="velnor-cold-verified-") as directory:
            descriptor = os.open("/bin/echo", os.O_RDONLY | os.O_CLOEXEC)
            try:
                duplicate = os.dup(descriptor)
                try:
                    expected = installer._descriptor_hash(duplicate)
                finally:
                    os.close(duplicate)
                output, status, _wall = process_owner.observe_install_child(
                    ["/bin/echo", "fd-ok"], {"PATH": "/usr/bin:/bin"}, directory,
                    4096, 5, executable_descriptor=descriptor)
            finally:
                os.close(descriptor)
        self.assertEqual((output, status), (b"fd-ok\n", 0))

    def test_malformed_compiled_stage_shape_is_rejected(self):
        malformed = _recipe()
        del malformed["stages"][0]["arguments"]
        with patch.object(recipe_owner, "_COMPILED_COLD_SDK_RECIPE", malformed), \
                patch.object(recipe_owner, "require_compiler_projection"), \
                self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_stage_source"):
            recipe_owner._compiled_recipe()

    def test_before_query_rejects_manager_settings_and_rustup_image_drift(self):
        root = "/fixture"
        manager = recipe_owner._MANAGER_SHA256
        paths = [
            *("cargo/bin/" + name for name in
              ("rustup", "cargo", "rustc", "rustdoc", "cargo-clippy", "clippy-driver",
               "rustfmt", "cargo-fmt")),
            "rustup-home/settings.toml",
            "rustup-home/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc",
        ]
        entries = [{"path": path, "kind": "file",
                    "sha256": manager if path.startswith("cargo/") else "s" * 64}
                   for path in paths]
        baseline = json.dumps({"schema": 3, "entries": entries},
                              separators=(",", ":")).encode()
        installed = {entry["path"]: entry for entry in entries}
        context = type("FixtureContext", (), {
            "root": root,
            "require_current": lambda self: None,
        })()
        recipe = {"manager_sha256": manager}
        def regular_hash(path, executable=True):
            return "s" * 64 if path.endswith("/rustup-home/settings.toml") else manager

        with patch.object(installer, "_regular_hash", side_effect=regular_hash), \
                patch("source_intent_cold_manifest.source_original_inventory",
                      return_value=type("Inventory", (), {"canonical_bytes": baseline})()):
            for target, error in (("cargo/bin/rustup", "cold_sdk_manager_proxy_binding"),
                                  ("rustup-home/settings.toml", "cold_sdk_settings_binding"),
                                  (paths[-1], "cold_sdk_compiler_image_changed")):
                changed = dict(installed[target], sha256="f" * 64)
                candidate = dict(installed, **{target: changed})
                with self.subTest(target=target), \
                        self.assertRaisesRegex(ColdSourceIntent, error):
                    installer._before_query(context, recipe, baseline, candidate)
            for extra in ("rustup-home/foreign", "cargo/bin/foreign"):
                candidate = dict(installed, **{extra: {"path": extra, "kind": "file",
                                                      "sha256": manager}})
                with self.subTest(extra=extra), \
                        self.assertRaisesRegex(ColdSourceIntent,
                                               "cold_sdk_compiler_image_changed"):
                    installer._before_query(context, recipe, baseline, candidate)
            for missing, error in (("cargo/bin/cargo", "cold_sdk_manager_proxy_binding"),
                                   ("rustup-home/settings.toml", "cold_sdk_settings_binding"),
                                   (paths[-1], "cold_sdk_compiler_image_changed")):
                candidate = dict(installed)
                del candidate[missing]
                with self.subTest(missing=missing), \
                        self.assertRaisesRegex(ColdSourceIntent, error):
                    installer._before_query(context, recipe, baseline, candidate)

    def test_missing_recipe_stops_load_before_any_stage(self):
        import source_intent_cold_sdk as sdk_owner
        with patch.object(recipe_owner, "_COMPILED_COLD_SDK_RECIPE", None), \
                patch.object(installer, "_run_stage") as run_stage, \
                self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_compiled_recipe_unavailable"):
            sdk_owner.load_source_intent_sdk()
        run_stage.assert_not_called()


if __name__ == "__main__":
    unittest.main()
