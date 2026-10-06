"""Candidate observation guards; synthetic contexts never qualify a compiler."""
import copy
import json
import os
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

ORCH_SOURCE = Path(__file__).resolve().parent
MISE_SOURCE = ORCH_SOURCE.parents[1] / "velnor-actions-mise" / "src"
sys.path.insert(0, str(MISE_SOURCE))
sys.path.insert(0, str(ORCH_SOURCE))

import source_root_rust_candidate_observe as candidate_observe
import source_root_rust_candidate_recipe_test as recipe_fixture
from source_intent_cold_common import ColdSourceIntent


def _recipe():
    return copy.deepcopy(recipe_fixture._recipe())


def _manifest(recipe, mutate=None):
    prefix = "rustup-home/toolchains/" + recipe["toolchain"] + "/"
    entries = [{"path": prefix + item["path"], "kind": "file",
                "mode": item["mode"], "sha256": item["sha256"]}
               for item in recipe["native_source_authority"]["payload"]]
    if mutate:
        mutate(entries, prefix)
    return json.dumps({"schema": 3, "entries": entries},
                      separators=(",", ":")).encode()


def _context():
    return SimpleNamespace(root="/fixture", require_current=lambda: None)


class CandidateObserveTests(unittest.TestCase):
    def test_payload_missing_extra_or_wrong_hash_denies_before_spawn(self):
        recipe = _recipe()
        cases = (
            ("missing", lambda entries, _prefix: entries.pop(),
             "root_candidate_payload_changed"),
            ("wrong-hash", lambda entries, _prefix: entries[0].__setitem__("sha256", "0" * 64),
             "root_candidate_payload_changed"),
            ("extra", lambda entries, prefix: entries.append(
                {"path": prefix + "bin/foreign", "kind": "file", "mode": 0o755,
                 "sha256": "0" * 64}), "root_candidate_extra_compiler"),
        )
        for label, mutate, reason in cases:
            with self.subTest(case=label):
                manifest = _manifest(recipe, mutate)
                sizes = [item["size"] for item in recipe["native_source_authority"]["payload"]]
                hashes = [item["sha256"] for item in recipe["native_source_authority"]["payload"]]
                descriptor = patch.object(
                    candidate_observe, "_binary_descriptor",
                    side_effect=lambda _path: os.open(os.devnull, os.O_RDONLY))
                fstat = patch.object(
                    candidate_observe.os, "fstat",
                    side_effect=lambda _fd: SimpleNamespace(st_size=sizes.pop(0)))
                digest = patch.object(candidate_observe, "_descriptor_hash",
                                      side_effect=lambda _fd, executable=True: hashes.pop(0))
                with patch.object(candidate_observe, "inventory_candidate_root",
                                  return_value=manifest), \
                        patch.object(candidate_observe, "_observe_candidate_verified") as spawned, \
                        descriptor, fstat, digest, \
                        self.assertRaisesRegex(ColdSourceIntent, reason):
                    candidate_observe._query(
                        _context(), recipe, manifest, {}, "/fixture/rustup", "d" * 64,
                        ["probe"], {}, 4096)
                spawned.assert_not_called()

    def test_original_pre_and_post_spawn_mutation_stops_query(self):
        recipe = _recipe()
        baseline, changed = b"baseline", b"changed"
        cases = (("pre", [changed], "root_candidate_pre_spawn_mutation", 0),
                 ("post", [baseline, changed], "root_candidate_post_spawn_mutation", 1))
        for phase, snapshots, reason, observed_calls in cases:
            with self.subTest(phase=phase), \
                    patch.object(candidate_observe, "inventory_candidate_root",
                                 side_effect=snapshots), \
                        patch.object(candidate_observe, "_validate_payload"), \
                    patch.object(candidate_observe, "_observe_candidate_verified",
                                 return_value=(b"ok", 0, 1)) as observed, \
                    self.assertRaisesRegex(ColdSourceIntent, reason):
                candidate_observe._query(_context(), recipe, baseline, {}, "/fixture/rustup",
                                         "d" * 64, ["probe"], {}, 4096)
            self.assertEqual(observed.call_count, observed_calls)


if __name__ == "__main__":
    unittest.main()
