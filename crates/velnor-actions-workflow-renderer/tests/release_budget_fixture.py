# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("budget", os.environ["BUDGET_SCRIPT"])
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)


class Response(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()


class Fixture(unittest.TestCase):
    def setUp(self):
        self.run = {"id": 123, "run_attempt": 1, "head_sha": "a" * 40, "repository": {"full_name": "example/repo"}}
        self.jobs = {"total_count": 1, "jobs": [{"id": 77, "run_id": 123, "run_attempt": 1, "head_sha": "a" * 40, "name": "Release publish", "status": "in_progress", "started_at": "2026-01-01T00:00:00Z"}]}
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = os.path.join(self.temp.name, "output")
        self.env = {"GITHUB_REPOSITORY": "example/repo", "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1", "GITHUB_JOB": "release-publish", "GITHUB_SHA": "a" * 40, "GH_TOKEN": "synthetic", "GITHUB_OUTPUT": self.path}

    def run_script(self):
        class Opener:
            def open(inner, request, timeout):
                data = self.jobs if "/jobs?" in request.full_url else self.run
                return Response(json.dumps(data).encode())
        with patch.dict(os.environ, self.env, clear=True), patch.object(budget.urllib.request, "build_opener", return_value=Opener()) as opener, patch.object(budget.time, "time_ns", return_value=1767225660 * 10**9), patch.object(budget.time, "CLOCK_BOOTTIME", 7, create=True), patch.object(budget.time, "clock_gettime_ns", return_value=100 * 10**9):
            budget.main()
            self.assertEqual(opener.call_args.args[0].proxies, {})
            self.assertIsInstance(opener.call_args.args[1], budget.NoRedirect)

    def test_valid_identity_and_original_start_determine_deadline(self):
        self.run_script()
        with open(self.path, encoding="utf-8") as result:
            self.assertEqual(result.read(), "budget=v1:123:1:release-publish:1840000000000\n")

    def test_rejects_duplicate_json_keys(self):
        with self.assertRaises(ValueError):
            json.loads('{"id":1,"id":2}', object_pairs_hook=budget.unique)

    def test_rejects_identity_inventory_and_time_tampering(self):
        for mutation in ("run", "head", "attempt", "name", "duplicate", "future", "expired", "missing_page"):
            with self.subTest(mutation=mutation):
                self.setUp()
                item = self.jobs["jobs"][0]
                if mutation == "run": self.run["id"] = 124
                if mutation == "head": item["head_sha"] = "b" * 40
                if mutation == "attempt": item["run_attempt"] = 2
                if mutation == "name": item["name"] = "another job"
                if mutation == "duplicate": self.jobs = {"total_count": 2, "jobs": [item, dict(item)]}
                if mutation == "future": item["started_at"] = "2027-01-01T00:00:00Z"
                if mutation == "expired": item["started_at"] = "2025-12-31T23:00:00Z"
                if mutation == "missing_page": self.jobs["total_count"] = 101
                with self.assertRaises(ValueError): self.run_script()
                self.assertFalse(os.path.exists(self.path))


if __name__ == "__main__":
    unittest.main()
