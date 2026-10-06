"""Executable admission tests against the fixed generated helper."""

from contextlib import contextmanager
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import unittest
import zipfile


SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_admission.py"
SPEC = importlib.util.spec_from_file_location("release_admission_under_test", SOURCE)
ADMISSION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ADMISSION)

REPO = "tailrocks/velnor-new"
OTHER_REPO = "tailrocks/other"
SHA = "a" * 40
OTHER_SHA = "b" * 40
BRANCH = "main"
WORKFLOW_ID = 17
RUN_ID = 42
RUN_ATTEMPT = 3
REPOSITORY_ID = 101
KEY = f"r{RUN_ID}-a{RUN_ATTEMPT}"


def zip_document(filename, payload):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(filename, payload)
    return output.getvalue()


def digest(blob):
    return "sha256:" + hashlib.sha256(blob).hexdigest()


def document_artifact(artifact_id, kind, filename, payload):
    blob = zip_document(filename, payload)
    return ({
        "id": artifact_id,
        "name": f"velnor-{kind}-{KEY}",
        "expired": False,
        "digest": digest(blob),
        "workflow_run": {
            "id": RUN_ID,
            "run_attempt": RUN_ATTEMPT,
            "head_sha": SHA,
            "repository_id": REPOSITORY_ID,
        },
    }, blob)


def valid_plan():
    return {
        "schema": 1,
        "run_key": KEY,
        "plan_id": "plan-" + KEY,
        "head": SHA,
        "event": "push",
        "trust": "trusted",
        "task_ids": ["task-covered", "task-execute"],
        "obligations": [
            {
                "task_id": "task-covered",
                "job_id": "rust-covered",
                "decision": "covered_by_trusted_baseline",
                "task_digest": "b3-" + "1" * 64,
                "input_digest": "b3-" + "2" * 64,
                "closure_digest": "b3-" + "3" * 64,
                "baseline_proof": {
                    "source_commit": "c" * 40,
                    "run_id": 501,
                    "artifact_id": 601,
                    "artifact_name": "velnor-baseline-r501-a1",
                    "manifest_digest": "b3-" + "4" * 64,
                },
            },
            {
                "task_id": "task-execute",
                "job_id": "rust-execute",
                "decision": "execute",
                "task_digest": "b3-" + "5" * 64,
                "input_digest": "b3-" + "6" * 64,
                "closure_digest": "b3-" + "7" * 64,
            },
        ],
    }


def valid_final():
    return {
        "schema": 1,
        "report_id": "final-" + KEY,
        "run_key": KEY,
        "plan_id": "plan-" + KEY,
        "status": "passed",
        "counts": {
            "selected": 2,
            "reused": 0,
            "executed": 1,
            "empty_partition": 0,
            "covered": 1,
            "failed": 0,
            "cancelled": 0,
            "blocked": 0,
            "not_run": 0,
        },
        "required_job_results": [
            {"job_id": "rust-covered", "conclusion": "skipped"},
            {"job_id": "rust-execute", "conclusion": "success"},
        ],
    }


class Fixture:
    """Small in-memory GitHub API with real ZIP payloads and metadata."""

    def __init__(self):
        self.run = {
            "id": RUN_ID,
            "run_attempt": RUN_ATTEMPT,
            "repository": {"id": REPOSITORY_ID, "full_name": REPO},
            "head_repository": {"id": REPOSITORY_ID, "full_name": REPO},
            "head_sha": SHA,
            "head_branch": BRANCH,
            "event": "push",
            "workflow_id": WORKFLOW_ID,
            "path": ".github/workflows/ci.yml",
            "status": "completed",
            "conclusion": "success",
        }
        self.jobs = [{
            "id": 71,
            "name": "Required",
            "run_id": RUN_ID,
            "run_attempt": RUN_ATTEMPT,
            "head_sha": SHA,
            "status": "completed",
            "conclusion": "success",
        }]
        plan = json.dumps(valid_plan(), sort_keys=True, separators=(",", ":")).encode()
        final = json.dumps(valid_final(), sort_keys=True, separators=(",", ":")).encode()
        final_artifact, final_blob = document_artifact(
            81, "final", "final-report.json", final
        )
        plan_artifact, plan_blob = document_artifact(82, "plan", "plan.json", plan)
        self.artifacts = [final_artifact, plan_artifact]
        self.blobs = {81: final_blob, 82: plan_blob}
        self.api_calls = []
        self.page_calls = []

    def api(self, path, binary=False):
        self.api_calls.append((path, binary))
        if binary:
            artifact_id = int(path.rstrip("/").split("/")[-2])
            return self.blobs[artifact_id]
        if path == f"repos/{REPO}":
            return {"default_branch": BRANCH}
        if path == f"repos/{REPO}/compare/{SHA}...{BRANCH}":
            return {"merge_base_commit": {"sha": SHA}, "status": "ahead"}
        if path == f"repos/{REPO}/actions/workflows/ci.yml":
            return {"id": WORKFLOW_ID, "path": ".github/workflows/ci.yml", "state": "active"}
        if path == f"repos/{REPO}/actions/runs/{RUN_ID}":
            return copy.deepcopy(self.run)
        raise AssertionError(f"unexpected API request: {path}")

    def pages(self, path, field):
        self.page_calls.append((path, field))
        if field == "workflow_runs":
            return [copy.deepcopy(self.run)]
        if field == "jobs":
            return copy.deepcopy(self.jobs)
        if field == "artifacts":
            return copy.deepcopy(self.artifacts)
        raise AssertionError(f"unexpected page field: {field}")

    def replace_document(self, kind, payload):
        filename = "final-report.json" if kind == "final" else "plan.json"
        artifact = next(item for item in self.artifacts if item["name"].startswith(f"velnor-{kind}-"))
        blob = zip_document(filename, payload)
        self.blobs[artifact["id"]] = blob
        artifact["digest"] = digest(blob)


@contextmanager
def patched_backend(fixture):
    old_api, old_pages = ADMISSION.api, ADMISSION.pages
    ADMISSION.api, ADMISSION.pages = fixture.api, fixture.pages
    try:
        yield
    finally:
        ADMISSION.api, ADMISSION.pages = old_api, old_pages


@contextmanager
def approved_environment(event="workflow_dispatch", ref=None, policy="rust"):
    values = {
        "APPROVED_REPOSITORY": REPO,
        "APPROVED_SOURCE_SHA": SHA,
        "APPROVED_DEFAULT_BRANCH": BRANCH,
        "GITHUB_REPOSITORY": REPO,
        "GITHUB_EVENT_NAME": event,
        "ADMISSION_EVENT_POLICY": policy,
        "GITHUB_REF": ref or "refs/heads/" + BRANCH,
    }
    previous = {key: os.environ.get(key) for key in values}
    os.environ.update(values)
    try:
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


class ReleaseAdmissionTest(unittest.TestCase):
    def assert_rejected(self, operation):
        with self.assertRaises(RuntimeError) as raised:
            operation()
        self.assertTrue(str(raised.exception).startswith("release_admission:"))

    def test_closed_caller_policies_preserve_actual_event(self):
        cases = [
            ("rust", "workflow_dispatch", "refs/heads/main", True),
            ("rust", "workflow_dispatch", "refs/tags/v1.0.0", False),
            ("default-branch", "schedule", "refs/heads/main", True),
            ("default-branch", "schedule", "refs/heads/other", False),
            ("oci-tag", "workflow_dispatch", "refs/tags/v1.0.0", True),
            ("oci-tag", "push", "refs/heads/main", False),
            ("desktop-tag", "push", "refs/tags/v1.0.0", True),
            ("desktop-tag", "workflow_dispatch", "refs/tags/v1.0.0", False),
            ("unknown", "push", "refs/tags/v1.0.0", False),
        ]
        for policy, event, ref, allowed in cases:
            with self.subTest(policy=policy, event=event, ref=ref), approved_environment(event, ref, policy):
                if allowed:
                    ADMISSION.caller_event(BRANCH)
                else:
                    self.assert_rejected(lambda: ADMISSION.caller_event(BRANCH))

    def test_main_accepts_exact_successful_ci_and_zip_proof(self):
        fixture = Fixture()
        with patched_backend(fixture), approved_environment():
            ADMISSION.main()
        self.assertIn(
            (f"repos/{REPO}/actions/artifacts/81/zip", True), fixture.api_calls
        )
        self.assertIn(
            (f"repos/{REPO}/actions/artifacts/82/zip", True), fixture.api_calls
        )

    def test_qualify_rejects_required_job_skipped_or_duplicated(self):
        for mutation in ("skipped", "duplicate"):
            with self.subTest(mutation=mutation):
                fixture = Fixture()
                if mutation == "skipped":
                    fixture.jobs[0]["conclusion"] = "skipped"
                else:
                    fixture.jobs.append(copy.deepcopy(fixture.jobs[0]))
                self.assert_rejected(
                    lambda: self._qualify(fixture, fixture.run)
                )

    def test_qualify_rejects_wrong_run_identity(self):
        mutations = {
            "head": lambda run: run.update(head_sha=OTHER_SHA),
            "repository": lambda run: run["repository"].update(full_name=OTHER_REPO),
            "head_repository": lambda run: run["head_repository"].update(full_name=OTHER_REPO),
            "event": lambda run: run.update(event="pull_request"),
            "path": lambda run: run.update(path=".github/workflows/other.yml"),
        }
        for name, mutate in mutations.items():
            with self.subTest(mutation=name):
                fixture = Fixture()
                run = copy.deepcopy(fixture.run)
                mutate(run)
                self.assert_rejected(lambda: self._qualify(fixture, run))

    def test_qualify_rejects_required_job_from_wrong_attempt(self):
        fixture = Fixture()
        fixture.jobs[0]["run_attempt"] = RUN_ATTEMPT + 1
        self.assert_rejected(lambda: self._qualify(fixture, fixture.run))

    def test_document_binding_rejects_wrong_head_or_repository(self):
        for field in ("head_sha", "repository_id"):
            with self.subTest(field=field):
                fixture = Fixture()
                if field == "head_sha":
                    fixture.artifacts[0]["workflow_run"][field] = OTHER_SHA
                else:
                    fixture.artifacts[0]["workflow_run"][field] = REPOSITORY_ID + 1
                self.assert_rejected(
                    lambda: self._evidence(fixture)
                )

    def test_evidence_rejects_no_work_and_empty_obligations(self):
        fixture = Fixture()
        final = valid_final()
        final["status"] = "no_work"
        fixture.replace_document(
            "final", json.dumps(final, sort_keys=True, separators=(",", ":")).encode()
        )
        self.assert_rejected(lambda: self._evidence(fixture))

        fixture = Fixture()
        plan = valid_plan()
        plan["task_ids"] = []
        plan["obligations"] = []
        final = valid_final()
        final["status"] = "no_work"
        final["counts"] = {
            "selected": 0,
            "reused": 0,
            "executed": 0,
            "empty_partition": 0,
            "covered": 0,
            "failed": 0,
            "cancelled": 0,
            "blocked": 0,
            "not_run": 0,
        }
        final["required_job_results"] = []
        fixture.replace_document(
            "plan", json.dumps(plan, sort_keys=True, separators=(",", ":")).encode()
        )
        fixture.replace_document(
            "final", json.dumps(final, sort_keys=True, separators=(",", ":")).encode()
        )
        self.assert_rejected(lambda: self._evidence(fixture))

    def test_evidence_rejects_inconsistent_counts(self):
        mutations = {
            "selected": 0,
            "reused": 1,
            "executed": 0,
            "covered": 0,
            "failed": 1,
            "not_run": 1,
        }
        for field, value in mutations.items():
            with self.subTest(field=field):
                fixture = Fixture()
                final = valid_final()
                final["counts"][field] = value
                fixture.replace_document(
                    "final", json.dumps(final, sort_keys=True, separators=(",", ":")).encode()
                )
                self.assert_rejected(lambda: self._evidence(fixture))

    def test_evidence_rejects_artifact_digest_tamper(self):
        fixture = Fixture()
        fixture.artifacts[0]["digest"] = "sha256:" + "0" * 64
        self.assert_rejected(lambda: self._evidence(fixture))

    def test_evidence_rejects_duplicate_json_key(self):
        fixture = Fixture()
        fixture.replace_document("final", b'{"schema":1,"schema":1}')
        self.assert_rejected(lambda: self._evidence(fixture))

    def _qualify(self, fixture, run):
        with patched_backend(fixture):
            return ADMISSION.qualify(run, REPO, SHA, BRANCH, WORKFLOW_ID)

    def _evidence(self, fixture):
        with patched_backend(fixture):
            return ADMISSION.evidence(fixture.run, REPO, SHA)


if __name__ == "__main__":
    unittest.main()
