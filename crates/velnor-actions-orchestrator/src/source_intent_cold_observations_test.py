"""Cold observation ordering and guards; private fixtures never qualify an SDK."""
import json
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_intent_cold_install as installer
import source_intent_cold_manifest as manifest_owner
import source_intent_cold_recipe as recipe_owner
from source_intent_cold_common import ColdSourceIntent


ROOT = "/fixture"
TOOLCHAIN = "1.98.1-x86_64-unknown-linux-gnu"
MANAGER = recipe_owner._MANAGER_SHA256
COMMANDS = tuple(recipe_owner._EXPECTED_OBLIGATIONS["commands"])


def _recipe():
    return {"host": "x86_64-unknown-linux-gnu", "rust_version": "1.98.1",
            "toolchain": TOOLCHAIN, "manager_sha256": MANAGER,
            "expected_obligations": recipe_owner._EXPECTED_OBLIGATIONS}


def _fixture_manifest():
    paths = ["cargo/bin/" + name for name in
             ("rustup", "cargo", "rustc", "rustdoc", "cargo-clippy", "clippy-driver",
              "rustfmt", "cargo-fmt")]
    paths += ["rustup-home/settings.toml"]
    paths += ["rustup-home/toolchains/" + TOOLCHAIN + "/bin/" + name for name in COMMANDS]
    entries = []
    for path in paths:
        executable = path != "rustup-home/settings.toml"
        entries.append({"path": path, "kind": "file", "mode": 0o755 if executable else 0o644,
                        "sha256": MANAGER if path.startswith("cargo/bin/") else "a" * 64})
    data = json.dumps({"schema": 3, "entries": entries}, separators=(",", ":")).encode()
    return data, {entry["path"]: entry for entry in entries}


def _context():
    return SimpleNamespace(root=ROOT, require_current=lambda: None)


class ColdObservationTests(unittest.TestCase):
    def test_guarded_query_checks_before_and_after_actual_query(self):
        baseline, _entries = _fixture_manifest()
        mutated = baseline + b" "
        context, recipe = _context(), _recipe()
        events = []

        def guard(_context, _recipe, observed, _installed):
            events.append("before" if observed == baseline else "after")
            if observed == mutated:
                raise ColdSourceIntent("fixture_after_query_mutation")

        def observe(*_args, **_kwargs):
            events.append("query")
            return b"ok", 0, 1

        with patch("source_intent_cold_manifest.source_original_inventory",
                   side_effect=[SimpleNamespace(canonical_bytes=mutated)]), \
                patch.object(installer, "_before_query", side_effect=guard), \
                patch.object(installer, "_observe_verified", side_effect=observe):
            with self.assertRaisesRegex(ColdSourceIntent, "fixture_after_query_mutation"):
                installer._guarded_query(context, recipe, baseline, _entries,
                                         ROOT + "/cargo/bin/rustup", MANAGER, ["probe"], {}, 4096)
        self.assertEqual(events, ["before", "query", "after"])

    def test_missing_or_wrong_obligation_stops_before_version_queries(self):
        manifest, entries = _fixture_manifest()
        recipe, context = _recipe(), _context()
        valid_components = ("\n".join(name + "-" + recipe["host"]
                                        for name in recipe["expected_obligations"]["components"])
                            + "\n").encode()
        cases = (("missing-component", b"cargo\n", b"unused\n", 1),
                 ("wrong-target", valid_components, b"wrong-target\n", 2))
        for label, component_output, target_output, calls in cases:
            with self.subTest(obligation=label):
                seen = []

                def guarded(_context, _recipe, _baseline, _installed, _path, _digest,
                            arguments, _environment, _limit):
                    seen.append(arguments)
                    output = component_output if arguments[0] == "component" else target_output
                    return output, manifest

                with patch("source_intent_cold_manifest.source_original_inventory",
                           return_value=SimpleNamespace(canonical_bytes=manifest)), \
                        patch.object(installer, "_guarded_query", side_effect=guarded), \
                        patch.object(installer, "_regular_hash", return_value="a" * 64):
                    with self.assertRaisesRegex(ColdSourceIntent, "cold_sdk_installed_obligations"):
                        installer._observe_tools(ROOT, recipe, {}, context)
                self.assertEqual(len(seen), calls)
                self.assertEqual(seen[0][0], "component")
                if label == "wrong-target":
                    self.assertEqual(seen[1][0], "target")

    def test_observe_tools_runs_two_obligations_then_seven_which_version_pairs(self):
        manifest, entries = _fixture_manifest()
        recipe, context = _recipe(), _context()
        calls = []
        paths = {command: ROOT + "/rustup-home/toolchains/" + TOOLCHAIN + "/bin/" + command
                 for command in COMMANDS}
        component_output = "\n".join(name + "-" + recipe["host"]
                                      for name in recipe["expected_obligations"]["components"]) + "\n"
        target_output = "x86_64-unknown-linux-gnu\n"

        def guarded(_context, _recipe, _baseline, _installed, path, _digest, arguments,
                    _environment, _limit):
            calls.append((path, arguments))
            if arguments[0] == "component":
                return component_output.encode(), manifest
            if arguments[0] == "target":
                return target_output.encode(), manifest
            if arguments[0] == "which":
                return (paths[arguments[-1]] + "\n").encode(), manifest
            command = arguments[-2] if arguments[0] == "run" else path.rsplit("/", 1)[-1]
            if command == "rustc":
                return b"rustc 1.98.1 (fixture)\nhost: x86_64-unknown-linux-gnu\n", manifest
            if command == "cargo":
                return b"cargo 1.98.1 (fixture)\n", manifest
            if command == "rustdoc":
                return b"rustdoc 1.98.1 (fixture)\n", manifest
            return (command + " auxiliary\n").encode(), manifest

        def regular_hash(path, executable=True):
            relative = path.removeprefix(ROOT + "/")
            return entries.get(relative, {}).get("sha256", MANAGER)

        with patch("source_intent_cold_manifest.source_original_inventory",
                   return_value=SimpleNamespace(canonical_bytes=manifest)), \
                patch.object(installer, "_guarded_query", side_effect=guarded), \
                patch.object(installer, "_regular_hash", side_effect=regular_hash):
            identities, tools, final, obligations = installer._observe_tools(
                ROOT, recipe, {}, context)
        self.assertEqual(final, manifest)
        self.assertEqual(set(identities), set(COMMANDS))
        self.assertEqual(set(tools), {"rustc", "cargo", "rustdoc"})
        self.assertEqual(obligations["targets"], target_output)
        self.assertEqual(len(calls), 16)
        self.assertEqual(calls[:2], [
            (ROOT + "/cargo/bin/rustup",
             ["component", "list", "--installed", "--toolchain", TOOLCHAIN]),
            (ROOT + "/cargo/bin/rustup",
             ["target", "list", "--installed", "--toolchain", TOOLCHAIN])])
        observed_commands = [arguments[-1] for _path, arguments in calls[2::2]]
        self.assertEqual(observed_commands, list(COMMANDS))
        self.assertEqual([path for path, _args in calls[3::2]][:3],
                         [paths[name] for name in COMMANDS[:3]])
        self.assertTrue(all(path == ROOT + "/cargo/bin/rustup"
                            for path, _args in calls[3::2][3:]))
        self.assertEqual([args for _path, args in calls[3::2]][:3],
                         [["--version", "--verbose"]] * 3)
        self.assertEqual([args for _path, args in calls[3::2][3:]],
                         [["run", TOOLCHAIN, command, "--version"]
                          for command in COMMANDS[3:]])


if __name__ == "__main__":
    unittest.main()
