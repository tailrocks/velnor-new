#!/usr/bin/env python3
"""Offline regression checks for source-bound CI timeline analysis."""

import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

import yaml


LOCATION = Path(__file__).with_name("analyze-ci-performance.py")
SPEC = importlib.util.spec_from_file_location("ci_performance_analysis", LOCATION)
ANALYZER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ANALYZER)


def job(identifier, start, end, conclusion="success"):
    return {
        "id": identifier, "name": str(identifier), "conclusion": conclusion,
        "started_at": f"2026-10-03T00:00:{start:02}Z",
        "completed_at": f"2026-10-03T00:00:{end:02}Z",
    }


class TimelineTests(unittest.TestCase):
    def test_linear_path_keeps_delay_separate_from_job_walls(self):
        result = ANALYZER.analyze(
            {"a": job(1, 0, 10), "b": job(2, 12, 20)},
            {"a": [], "b": ["a"]},
        )
        self.assertEqual(result["completion_path_seconds"], 20)
        self.assertEqual(result["longest_job_wall_dependency_path_seconds"], 18)
        self.assertEqual(result["runner_wall_sum_seconds"], 18)
        self.assertEqual(result["observed_completion_path"][1]
                         ["dependency_start_delay_seconds"], 2)

    def test_skipped_bridge_preserves_executed_ancestor(self):
        result = ANALYZER.analyze(
            {"a": job(1, 0, 20), "b": job(2, 0, 0, "skipped"),
             "c": job(3, 22, 30)},
            {"a": [], "b": ["a"], "c": ["b"]},
        )
        self.assertEqual([row["workflow_job"] for row in
                          result["observed_completion_path"]], ["a", "c"])
        self.assertEqual(result["longest_job_wall_dependency_path_seconds"], 28)

    def test_skipped_bridge_cannot_hide_predecessor_overlap(self):
        with self.assertRaisesRegex(ValueError, "predecessor"):
            ANALYZER.analyze(
                {"a": job(1, 0, 20), "b": job(2, 0, 0, "skipped"),
                 "c": job(3, 10, 30)},
                {"a": [], "b": ["a"], "c": ["b"]},
            )

    def test_timestamp_overlap_is_recorded_and_subtracted(self):
        result = ANALYZER.analyze(
            {"a": job(1, 0, 20), "b": job(2, 19, 30)},
            {"a": [], "b": ["a"]},
        )
        self.assertEqual(result["completion_path_seconds"], 30)
        self.assertEqual(result["observed_completion_path"][1]
                         ["timestamp_overlap_seconds"], 1)
        self.assertEqual(result["api_timestamp_resolution_seconds"], 1)

    def test_cycle_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "cycle"):
            ANALYZER.analyze({"a": job(1, 0, 10), "b": job(2, 12, 20)},
                             {"a": ["b"], "b": ["a"]})

    def test_timestamps_require_text_and_timezone(self):
        for value in (None, 0, {}, "2026-10-03T00:00:00"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                ANALYZER.timestamp(value)

    def test_unobserved_process_measurements_remain_unknown(self):
        result = ANALYZER.analyze({"a": job(1, 0, 10)}, {"a": []})
        for metric in ("compiler_process_seconds", "link_seconds",
                       "pure_queue_seconds", "runner_provision_seconds"):
            with self.subTest(metric=metric):
                self.assertIsNone(result[metric])


class DocumentTests(unittest.TestCase):
    def test_duplicate_json_keys_are_rejected(self):
        with patch.object(ANALYZER.COLLECTOR, "read", return_value=b'{"a":1,"a":2}'):
            with self.assertRaisesRegex(ValueError, "duplicate"):
                ANALYZER.document(Path("unused.json"))

    def test_duplicate_yaml_keys_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            yaml.load("jobs:\n  a: {}\n  a: {}", Loader=ANALYZER.StrictYaml)

    def test_yaml_boolean_rules_preserve_static_names(self):
        for name in ("on", "yes"):
            with self.subTest(name=name):
                workflow = yaml.load(f"jobs:\n  {name}: {{}}",
                                     Loader=ANALYZER.StrictYaml)
                actual, _ = ANALYZER.graph(workflow, [{"name": name}])
                self.assertIn(name, actual)
        self.assertIs(yaml.safe_load("on"), True)
        self.assertIs(yaml.load("true", Loader=ANALYZER.StrictYaml), True)

    def test_invalid_mapping_shapes_are_rejected(self):
        for workflow in (None, {"jobs": []}, {"jobs": {"a": None}}):
            with self.subTest(workflow=workflow), self.assertRaises(ValueError):
                ANALYZER.graph(workflow, [])

    def test_dynamic_and_malformed_dependencies_are_rejected(self):
        definitions = (
            {"needs": {}}, {"needs": "${{ inputs.jobs }}"},
            {"name": "${{ inputs.name }}"},
            {"strategy": {"matrix": {"x": [1, 2]}}},
            {"needs": ["a", "a"]},
        )
        for definition in definitions:
            with self.subTest(definition=definition), self.assertRaises(ValueError):
                ANALYZER.graph({"jobs": {"a": definition}}, [{"name": "a"}])


class SourceBindingTests(unittest.TestCase):
    def setUp(self):
        self.content = b"jobs:\n  a: {}\n"
        self.run = {"path": ".github/workflows/ci.yml", "head_sha": "a" * 40}
        self.response = {
            "path": self.run["path"], "encoding": "base64",
            "content": base64.b64encode(self.content).decode(),
            "sha": hashlib.sha1(b"blob " + str(len(self.content)).encode()
                                + b"\0" + self.content).hexdigest(),
        }

    def lookup(self, response):
        with patch.object(ANALYZER.COLLECTOR, "api", return_value=json.dumps(response)) as api:
            result = ANALYZER.workflow_for(self.run, "owner/repository")
            api.assert_called_once_with(
                "repos/owner/repository/contents/.github/workflows/ci.yml?ref="
                + self.run["head_sha"])
            return result

    def test_exact_source_lookup_binds_git_blob(self):
        _, content, path, blob = self.lookup(self.response)
        self.assertEqual(content, self.content)
        self.assertEqual(path, self.run["path"])
        self.assertEqual(blob, self.response["sha"])

    def test_forged_git_blob_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.lookup({**self.response, "sha": "b" * 40})

    def test_forged_source_path_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "response mismatch"):
            self.lookup({**self.response, "path": ".github/workflows/other.yml"})


if __name__ == "__main__":
    unittest.main()
