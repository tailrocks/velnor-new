"""Documented REST shapes in fixtures; no hosted execution evidence is claimed."""

import copy
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).parent))
import source_qualification_execution as execution


def fixture(event="workflow_dispatch", branch="main"):
    sha = "a" * 40
    environment = {"GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_SHA": sha,
                   "GITHUB_REF": "refs/heads/" + branch, "GITHUB_EVENT_NAME": event,
                   "GITHUB_WORKFLOW_SHA": sha,
                   "GITHUB_WORKFLOW_REF": execution.REPOSITORY + "/" + execution.WORKFLOW_PATH + "@refs/heads/" + branch}
    repository = {"id": 444, "full_name": execution.REPOSITORY, "default_branch": "main"}
    run = {"id": 123, "run_attempt": 2, "head_sha": sha, "event": event, "head_branch": branch,
           "path": execution.WORKFLOW_PATH, "workflow_id": 555,
           "repository": {"id": 444, "full_name": execution.REPOSITORY},
           "head_repository": {"id": 444, "full_name": execution.REPOSITORY},
           "status": "in_progress", "conclusion": None}
    workflow = {"id": 555, "path": execution.WORKFLOW_PATH, "state": "active", "name": "Owned tools"}
    documents = {execution.PREFIX: repository,
                 execution.PREFIX + "/actions/runs/123": run,
                 execution.PREFIX + "/actions/workflows/555": workflow}
    return environment, documents


def reader(documents):
    return Mock(side_effect=lambda endpoint: json.dumps(documents[endpoint], sort_keys=True).encode())


class ExecutionTests(unittest.TestCase):
    def test_canonical_dispatch_and_literal_candidate_push_are_admitted(self):
        for event, branch in (("workflow_dispatch", "main"), ("push", execution.CANDIDATE_BRANCH)):
            env, documents = fixture(event, branch)
            read = reader(documents)
            raw_documents = {}
            receipt = execution.admit_execution(read, env, api_documents=raw_documents)
            self.assertEqual(receipt["commit"], env["GITHUB_SHA"])
            self.assertEqual(receipt["head_branch"], branch)
            self.assertEqual(receipt["event"], event)
            self.assertEqual(receipt["workflow"]["sha"], env["GITHUB_SHA"])
            self.assertEqual(receipt["repository_id"], 444)
            self.assertEqual(receipt["workflow"]["id"], 555)
            self.assertEqual(set(receipt), execution.RECEIPT_FIELDS)
            self.assertEqual([call.args[0] for call in read.call_args_list], list(documents))
            for key, endpoint in (("repository", execution.PREFIX),
                                  ("run", execution.PREFIX + "/actions/runs/123"),
                                  ("workflow", execution.PREFIX + "/actions/workflows/555")):
                raw = json.dumps(documents[endpoint], sort_keys=True).encode()
                self.assertEqual(receipt["api_sha256"][key], execution.digest(raw))
                self.assertEqual(raw_documents[key], raw)
            self.assertNotIn("approval", receipt)
            self.assertNotIn("qualification", receipt)
            execution.validate_execution_receipt(receipt, env)
            execution.validate_execution_evidence(receipt, raw_documents, env)

    def test_documented_short_branch_path_suffix_is_closed(self):
        env, documents = fixture("push", execution.CANDIDATE_BRANCH)
        run = documents[execution.PREFIX + "/actions/runs/123"]
        run["path"] += "@" + execution.CANDIDATE_BRANCH
        execution.admit_execution(reader(documents), env)
        for path in (execution.WORKFLOW_PATH + "@main", execution.WORKFLOW_PATH + "@refs/heads/owned-tool-candidates",
                     ".github/workflows/other.yml", "../" + execution.WORKFLOW_PATH):
            run["path"] = path
            with self.subTest(path=path), self.assertRaises(ValueError):
                execution.admit_execution(reader(documents), env)

    def test_untrusted_event_ref_workflow_environment_rejected_before_api(self):
        env, _ = fixture()
        changes = [{"GITHUB_EVENT_NAME": "pull_request"}, {"GITHUB_EVENT_NAME": "pull_request_target"},
                   {"GITHUB_EVENT_NAME": "push"}, {"GITHUB_REF": "refs/tags/main"},
                   {"GITHUB_WORKFLOW_SHA": "b" * 40}, {"GITHUB_SHA": "0" * 40},
                   {"GITHUB_WORKFLOW_REF": env["GITHUB_WORKFLOW_REF"].replace("tailrocks", "fork")},
                   {"GITHUB_WORKFLOW_REF": env["GITHUB_WORKFLOW_REF"].replace("owned-tools", "other")},
                   {"GITHUB_RUN_ID": "123?event=push"}, {"GITHUB_RUN_ATTEMPT": "0"},
                   {"GITHUB_REF": "refs/heads/owned-tool-candidates"}, {"GITHUB_SHA": False}]
        for change in changes:
            read = Mock()
            with self.subTest(change=change), self.assertRaises(ValueError):
                execution.admit_execution(read, {**env, **change})
            read.assert_not_called()
        env, _ = fixture("push", "other-branch")
        with self.assertRaises(ValueError):
            execution.admit_execution(Mock(), env)

    def test_api_run_and_source_identity_must_match_exact_environment(self):
        env, baseline = fixture()
        endpoint = execution.PREFIX + "/actions/runs/123"
        changes = [{"id": 124}, {"id": True}, {"run_attempt": 1}, {"run_attempt": True},
                   {"head_sha": "b" * 40}, {"head_branch": "owned-tool-candidates"}, {"event": "push"},
                   {"workflow_id": 0}, {"workflow_id": True},
                   {"repository": {"id": 444, "full_name": "attacker/velnor-new"}},
                   {"repository": {"id": 445, "full_name": execution.REPOSITORY}},
                   {"head_repository": {"id": 999, "full_name": execution.REPOSITORY}},
                   {"head_repository": None}]
        for change in changes:
            documents = copy.deepcopy(baseline)
            documents[endpoint].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                execution.admit_execution(reader(documents), env)

    def test_api_canonical_repository_default_and_workflow_are_independent(self):
        env, baseline = fixture()
        mutations = [(execution.PREFIX, {"full_name": "fork/velnor-new"}),
                     (execution.PREFIX, {"id": True}), (execution.PREFIX, {"default_branch": "master"}),
                     (execution.PREFIX + "/actions/workflows/555", {"id": 556}),
                     (execution.PREFIX + "/actions/workflows/555", {"path": ".github/workflows/other.yml"}),
                     (execution.PREFIX + "/actions/workflows/555", {"path": execution.WORKFLOW_PATH + "@main"})]
        for endpoint, change in mutations:
            documents = copy.deepcopy(baseline)
            documents[endpoint].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                execution.admit_execution(reader(documents), env)
        env, documents = fixture("push", execution.CANDIDATE_BRANCH)
        documents[execution.PREFIX]["default_branch"] = execution.CANDIDATE_BRANCH
        with self.assertRaises(ValueError):
            execution.admit_execution(reader(documents), env)

    def test_emitted_receipt_rejects_aliases_unknowns_and_missing_api_digests(self):
        env, documents = fixture()
        receipt = execution.admit_execution(reader(documents), env)
        changes = [{"unknown": True}, {"schema": True}, {"run_attempt": "1"},
                   {"repository_id": True}, {"commit": "b" * 40}, {"default_branch": "other"},
                   {"workflow": {**receipt["workflow"], "sha": "b" * 40}},
                   {"workflow": {**receipt["workflow"], "unknown": True}},
                   {"api_sha256": {"repository": "a" * 64, "run": "b" * 64}},
                   {"api_sha256": {**receipt["api_sha256"], "workflow": "0" * 64}}]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                execution.validate_execution_receipt({**receipt, **change}, env)

    def test_read_api_scope_size_bytes_and_duplicate_json_are_bounded(self):
        read = Mock(return_value=b'{}')
        with self.assertRaises(ValueError):
            execution.read_document(read, "https://attacker.example/run")
        read.assert_not_called()
        for raw in ("string not bytes", b"", b"[]", b'{"id":1,"id":2}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                execution.read_document(Mock(return_value=raw), execution.PREFIX)
        with unittest.mock.patch.object(execution, "MAX_API", 1), self.assertRaises(ValueError):
            execution.read_document(Mock(return_value=b'{}'), execution.PREFIX)

    def test_durable_api_evidence_replays_without_headers_or_tokens(self):
        env, documents = fixture()
        env["GH_TOKEN"] = "must-not-enter-evidence"
        raw_documents = {}
        receipt = execution.admit_execution(reader(documents), env, api_documents=raw_documents)
        self.assertEqual(set(raw_documents), {"repository", "run", "workflow"})
        self.assertNotIn(b"must-not-enter-evidence", b"".join(raw_documents.values()))
        for changed in ({**raw_documents, "extra": b"{}"}, {**raw_documents, "run": b"{}"},
                        {key: value + b" " for key, value in raw_documents.items()}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                execution.validate_execution_evidence(receipt, changed, env)
        with self.assertRaises(ValueError):
            execution.admit_execution(reader(documents), env, api_documents={"existing": b"{}"})


if __name__ == "__main__":
    unittest.main()
