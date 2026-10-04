"""Actual candidate stage waits run only inside an isolated process capsule."""
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_root_rust_candidate_install as candidate_install
import source_root_rust_candidate_recipe as candidate_recipe
import source_root_rust_candidate_install_test as install_fixture


def _run_actual_stage_fixture():
    recipe = install_fixture._recipe()
    events = []
    with tempfile.TemporaryDirectory(prefix="root-rust-candidate-") as directory:
        temp = Path(os.path.realpath(directory))
        root = temp / recipe["namespace"]
        root.mkdir(parents=True)
        ca_file = temp / "ca.pem"
        ca_file.write_bytes(b"fixture-ca\n")
        owner, binding = install_fixture._foundation_fixture(str(root), events, str(ca_file))
        real_stage = candidate_install._run_stage

        def run_stage(stage, actual_environment):
            events.append(("stage-start", stage["name"], stage["source_sha256"]))
            phase = real_stage(stage, actual_environment)
            if stage["name"] == "acquire":
                install_fixture._acquired_layout(root)
            elif stage["name"] == "install":
                install_fixture._installed_layout(root)
            events.append(("stage-done", stage["name"]))
            return phase

        def regular_hash(path, executable=True):
            del executable
            events.append(("hash", path))
            return recipe["manager_sha256"]

        with binding, \
                patch.object(candidate_install, "_compiled_candidate_recipe",
                             return_value=recipe), \
                patch.dict(candidate_recipe.os.environ, {
                    "RUNNER_TEMP": str(temp), candidate_recipe._ROOT_ENV: str(root)},
                           clear=False), \
                patch.object(candidate_recipe.platform, "system", return_value="Linux"), \
                patch.object(candidate_recipe.platform, "machine", return_value="x86_64"), \
                patch.object(candidate_install, "_run_stage", side_effect=run_stage), \
                patch.object(candidate_install, "_regular_hash", side_effect=regular_hash):
            witness = candidate_install.execute_root_rust_candidate()
        return {
            "phases": [dict(phase) for phase in witness._phases],
            "events": events,
            "root_entries": sorted(entry.name for entry in root.iterdir()),
            "cargo_entries": sorted(entry.name for entry in (root / "cargo-home").iterdir()),
            "rustup_home": sorted(entry.name for entry in (root / "rustup-home").iterdir()),
            "bootstrap_entries": sorted(entry.name for entry in
                                         (root / "rustup-bootstrap").iterdir()),
            "manager_target": os.readlink(root / "manager-bin" / "rustup"),
            "manager_path": str(root / "cargo-home" / "bin" / "rustup"),
            "raw_path": str(root / "rustup-bootstrap" / "rustup-init"),
            "root_leaves": list(candidate_recipe._LEAVES),
        }


class CandidateStageTests(unittest.TestCase):
    def test_actual_bash_stages_use_capsule_and_phase_proofs(self):
        script = ("import json; from source_root_rust_candidate_stage_test import "
                  "_run_actual_stage_fixture; print(json.dumps(_run_actual_stage_fixture()))")
        process = subprocess.Popen(
            [sys.executable, "-c", script], cwd=str(ORCH_SOURCE),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=30)
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
        self.assertEqual(process.returncode, 0, stderr)
        result = json.loads(stdout)
        recipe = install_fixture._recipe()
        self.assertEqual([phase["name"] for phase in result["phases"]],
                         ["clear", "acquire", "install"])
        self.assertEqual([phase["source_sha256"] for phase in result["phases"]],
                         [stage["source_sha256"] for stage in recipe["stages"]])
        self.assertTrue(all(phase["exit_code"] == 0 for phase in result["phases"]))
        events = result["events"]
        for name, stage in zip(("clear", "acquire", "install"), recipe["stages"]):
            start = events.index(["stage-start", name, stage["source_sha256"]])
            done = events.index(["stage-done", name])
            self.assertEqual(events[start - 1][0], "foundation")
            self.assertEqual(events[done + 1][0], "foundation")
        acquire_done = events.index(["stage-done", "acquire"])
        install_start = next(index for index, event in enumerate(events)
                             if event[0] == "stage-start" and event[1] == "install")
        hashes = [index for index, event in enumerate(events)
                  if event[0] == "hash" and event[1] == result["raw_path"]]
        self.assertTrue(any(acquire_done < index < install_start for index in hashes))
        native = [index for index, event in enumerate(events) if event[0] == "native"]
        self.assertEqual(len(native), 3)
        self.assertTrue(acquire_done < native[0] <= native[1] < install_start)
        compiler = [index for index, event in enumerate(events) if event[0] == "compiler"]
        self.assertTrue(compiler and compiler[0] < install_start)
        install_done = events.index(["stage-done", "install"])
        installed = [index for index, event in enumerate(events) if event[0] == "installed"]
        self.assertTrue(len(installed) >= 2 and installed[0] > install_done)
        self.assertEqual(result["root_entries"], sorted(result["root_leaves"]))
        self.assertEqual(result["cargo_entries"], ["bin"])
        self.assertEqual(result["rustup_home"], [])
        self.assertEqual(result["bootstrap_entries"], ["rustup-init"])
        self.assertEqual(result["manager_target"], result["manager_path"])


if __name__ == "__main__":
    unittest.main()
