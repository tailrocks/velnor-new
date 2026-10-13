#!/usr/bin/env python3
"""Offline regression checks for source-bound CI timeline analysis."""

import base64
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
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


class MainAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-analysis-test-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.source = "a" * 40
        self.documents = {
            "summary.json": {"schema": 1, "repository": "tailrocks/velnor-new",
                             "run_id": 7, "attempt": 1, "source_commit": self.source},
            "run.json": {"repository": {"full_name": "tailrocks/velnor-new"},
                         "id": 7, "run_attempt": 1, "head_sha": self.source,
                         "status": "completed", "conclusion": "failure",
                         "path": ".github/workflows/ci.yml"},
            "jobs.json": [{"total_count": 1, "jobs": [{
                **job(1, 0, 10, "failure"), "name": "a", "run_id": 7,
                "run_attempt": 1, "head_sha": self.source, "status": "completed",
            }]}],
        }
        self.content = b"jobs:\n  a: {}\n  skipped: {}\n"

    def invoke(self, documents, expected, source_jobs=1):
        for name, value in documents.items():
            (self.directory / name).write_text(json.dumps(value))
        for name in ("workflow-source.yml", "timeline-analysis.json"):
            (self.directory / name).unlink(missing_ok=True)
        content = self.content if source_jobs == 2 else b"jobs:\n  a: {}\n"
        response = {"path": ".github/workflows/ci.yml", "encoding": "base64",
                    "content": base64.b64encode(content).decode(),
                    "sha": hashlib.sha1(b"blob " + str(len(content)).encode()
                                        + b"\0" + content).hexdigest()}
        with patch("sys.argv", [str(LOCATION), str(self.directory)]), \
                patch("sys.stdout", new=io.StringIO()), \
                patch("sys.stderr", new=io.StringIO()), \
                patch.object(ANALYZER.COLLECTOR, "api", return_value=json.dumps(response)) as api:
            self.assertEqual(ANALYZER.main(), expected)
        if expected:
            api.assert_not_called()
            self.assertFalse((self.directory / "timeline-analysis.json").exists())
            self.assertFalse((self.directory / "workflow-source.yml").exists())
        else:
            api.assert_called_once()

    def test_failed_completed_run_is_measurable_without_performance_verdict(self):
        self.invoke(self.documents, 0)
        result = json.loads((self.directory / "timeline-analysis.json").read_text())
        self.assertEqual(result["completion_path_seconds"], 10)
        self.assertIsNone(result["compiler_process_seconds"])
        self.assertNotIn("qualified", result)

    def test_summary_schema_and_identity_sanity(self):
        replacements = (
            ("schema", 2), ("schema", True), ("run_id", 0), ("run_id", True),
            ("attempt", -1), ("attempt", True), ("source_commit", "bad"),
            ("repository", "tailrocks/other"),
        )
        for key, value in replacements:
            with self.subTest(key=key, value=value):
                documents = copy.deepcopy(self.documents)
                documents["summary.json"][key] = value
                self.invoke(documents, 1)

    def test_run_identity_mismatch_and_nonterminal_status(self):
        replacements = (("id", 8), ("id", True), ("run_attempt", 2),
                        ("run_attempt", True), ("head_sha", "b" * 40),
                        ("status", "in_progress"),
                        ("repository", {"full_name": "tailrocks/other"}))
        for key, value in replacements:
            with self.subTest(key=key, value=value):
                documents = copy.deepcopy(self.documents)
                documents["run.json"][key] = value
                self.invoke(documents, 1)

    def test_job_identity_mismatch(self):
        for key, value in (("id", 0), ("id", True), ("run_id", 8),
                           ("run_attempt", 2), ("head_sha", "b" * 40)):
            with self.subTest(key=key, value=value):
                documents = copy.deepcopy(self.documents)
                documents["jobs.json"][0]["jobs"][0][key] = value
                self.invoke(documents, 1)

    def test_nonterminal_null_and_unknown_job_outcomes_fail_before_api(self):
        for status, conclusion in (("in_progress", None), ("queued", "success"),
                                   ("completed", None), ("completed", "unknown"),
                                   ("completed", "stale"), ("completed", "startup_failure")):
            with self.subTest(status=status, conclusion=conclusion):
                documents = copy.deepcopy(self.documents)
                documents["jobs.json"][0]["jobs"][0].update(
                    status=status, conclusion=conclusion)
                self.invoke(documents, 1)

    def test_supported_terminal_outcomes_preserve_failed_analysis(self):
        for conclusion in ANALYZER.JOB_CONCLUSIONS - {"skipped"}:
            with self.subTest(conclusion=conclusion):
                documents = copy.deepcopy(self.documents)
                documents["jobs.json"][0]["jobs"][0]["conclusion"] = conclusion
                self.invoke(documents, 0)

    def test_skipped_terminal_job_needs_no_runner_timestamps(self):
        documents = copy.deepcopy(self.documents)
        documents["jobs.json"][0]["total_count"] = 2
        documents["jobs.json"][0]["jobs"].append({
            "id": 2, "name": "skipped", "status": "completed", "conclusion": "skipped",
            "run_id": 7, "run_attempt": 1, "head_sha": self.source,
            "started_at": None, "completed_at": None,
        })
        self.invoke(documents, 0, source_jobs=2)

    def test_missing_duplicate_and_malformed_job_pages_fail_before_api(self):
        first = copy.deepcopy(self.documents["jobs.json"][0])
        malformed = ([], [{**first, "total_count": 2}], [first, first],
                     [{**first, "total_count": True}], [{**first, "jobs": {}}],
                     [{**first, "jobs": [None]}])
        for pages in malformed:
            with self.subTest(pages=pages):
                documents = copy.deepcopy(self.documents)
                documents["jobs.json"] = pages
                self.invoke(documents, 1)


if __name__ == "__main__":
    unittest.main()
