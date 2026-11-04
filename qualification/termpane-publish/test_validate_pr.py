# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location("candidate_admission", pathlib.Path(__file__).with_name("validate-pr.py"))
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)
SHA = "a" * 40


def fixture():
    context = {"__typename": "CheckRun", "name": "Required", "conclusion": "SUCCESS", "checkSuite": {"app": {"databaseId": 15368}, "commit": {"oid": SHA}, "workflowRun": {"file": {"path": ".github/workflows/ci.yml", "repositoryName": "tailrocks/velnor-new"}}}}
    commit = {"oid": SHA, "statusCheckRollup": {"state": "SUCCESS", "contexts": {"pageInfo": {"hasNextPage": False}, "nodes": [context]}}}
    pr = {"number": 123, "state": "OPEN", "isDraft": False, "baseRefName": "main", "baseRepository": {"nameWithOwner": "tailrocks/velnor-new"}, "headRepository": {"nameWithOwner": "tailrocks/velnor-new"}, "headRefOid": SHA, "mergeStateStatus": "CLEAN", "reviewThreads": {"pageInfo": {"hasNextPage": False}, "nodes": [{"isResolved": True}]}, "commits": {"nodes": [{"commit": commit}]}}
    response = {"data": {"repository": {"pullRequest": pr}}}
    rules = [{"type": "required_status_checks", "parameters": {"strict_required_status_checks_policy": True, "required_status_checks": [{"context": "Required"}]}}, {"type": "pull_request", "parameters": {"required_review_thread_resolution": True}}]
    return response, rules


class CandidateAdmission(unittest.TestCase):
    def test_admits_exact_current_reviewable_head(self):
        response, rules = fixture()
        self.assertTrue(validator.admitted(SHA, "123", response, rules))

    def test_rejects_incomplete_or_ineligible_candidate(self):
        for case in ("stale", "fork", "base", "pending", "required", "app", "contexts", "threads", "unresolved", "rules", "draft", "null", "closed", "commit", "error", "missing", "empty_rules", "workflow", "workflow_repo", "suite_commit"):
            with self.subTest(case=case):
                response, rules = fixture()
                pr = response["data"]["repository"]["pullRequest"]
                commit = pr["commits"]["nodes"][0]["commit"]
                rollup = commit["statusCheckRollup"]
                if case == "stale": pr["headRefOid"] = "b" * 40
                if case == "fork": pr["headRepository"]["nameWithOwner"] = "other/repo"
                if case == "base": pr["baseRefName"] = "other"
                if case == "pending": rollup["state"] = "PENDING"
                if case == "required": rollup["contexts"]["nodes"][0]["conclusion"] = "FAILURE"
                if case == "app": rollup["contexts"]["nodes"][0]["checkSuite"]["app"]["databaseId"] = 1
                if case == "contexts": rollup["contexts"]["pageInfo"]["hasNextPage"] = True
                if case == "threads": pr["reviewThreads"]["pageInfo"]["hasNextPage"] = True
                if case == "unresolved": pr["reviewThreads"]["nodes"][0]["isResolved"] = False
                if case == "rules": rules[0]["parameters"]["strict_required_status_checks_policy"] = False
                if case == "draft": pr["isDraft"] = True
                if case == "null": pr["reviewThreads"]["pageInfo"]["hasNextPage"] = None
                if case == "closed": pr["state"] = "CLOSED"
                if case == "commit": commit["oid"] = "b" * 40
                if case == "error": response["errors"] = [{"message": "partial"}]
                if case == "missing": rollup["contexts"]["nodes"] = []
                if case == "empty_rules": rules.clear()
                if case == "workflow": rollup["contexts"]["nodes"][0]["checkSuite"]["workflowRun"]["file"]["path"] = ".github/workflows/other.yml"
                if case == "workflow_repo": rollup["contexts"]["nodes"][0]["checkSuite"]["workflowRun"]["file"]["repositoryName"] = "other/repo"
                if case == "suite_commit": rollup["contexts"]["nodes"][0]["checkSuite"]["commit"]["oid"] = "b" * 40
                self.assertFalse(validator.admitted(SHA, "123", response, rules))
        self.assertFalse(validator.admitted("A" * 40, "123", *fixture()))
        self.assertFalse(validator.admitted(SHA, "0", *fixture()))


if __name__ == "__main__":
    unittest.main()
