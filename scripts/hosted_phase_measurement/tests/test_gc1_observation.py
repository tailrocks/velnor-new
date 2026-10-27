"""Owned observation evidence must be accepted only when fully bound.

Verified measurements (API-bound ZIP, source/run/job/actual-attempt binding)
are accepted; unbound zeros and forged effective attempts are rejected.
Uncovered task reports stay explicitly unknown, never zero.
"""

import base64
import hashlib
import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from hosted_phase_measurement import measure_hosted_run as R
from hosted_phase_measurement import measure_support as S
from hosted_phase_measurement import report_counters as C
from hosted_phase_measurement import report_events as E

REPOSITORY = "o/r"
RUN_ID = 4242
ATTEMPT = 1
RUN_KEY = f"r{RUN_ID}-a{ATTEMPT}"
HEAD_SHA = "0123456789abcdef0123456789abcdef01234567"
OTHER_SHA = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
JOB_NAME = "unit-job"
JOB_ID = 101
ARTIFACT_ID = 777
MATRIX_KEY = "m-aaaaaaaaaaaaaaaa"
TASK_REPORT_ID = f"task-{RUN_KEY}-m-aaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb"
MEMBER = f"{MATRIX_KEY}/tasks/{TASK_REPORT_ID}{S.OBSERVATION_SUFFIX}"
SESSION = "gc1-t37-session"


def _run_doc():
    return {"id": RUN_ID, "run_attempt": ATTEMPT, "head_sha": HEAD_SHA}


def _jobs_doc(attempt=ATTEMPT, name=JOB_NAME, numeric_id=JOB_ID, sha=HEAD_SHA):
    job = {"id": numeric_id, "run_id": RUN_ID, "run_attempt": attempt,
           "head_sha": sha, "name": name,
           "run_url": f"https://api.github.com/repos/{REPOSITORY}/actions/runs/{RUN_ID}",
           "html_url": f"https://github.com/{REPOSITORY}/actions/runs/{RUN_ID}/job/{numeric_id}",
           "status": "completed", "conclusion": "success"}
    return {"total_count": 1, "jobs": [job]}


def _binding(**overrides):
    binding = {"schema": 1, "task_report_id": TASK_REPORT_ID,
               "artifact_id": f"velnor-crate-{RUN_KEY}-{JOB_NAME}",
               "member_name": MEMBER, "member_size_bytes": 512,
               "member_digest": "b3-" + "ab" * 32,
               "producer_job": JOB_NAME, "producer_attempt": ATTEMPT,
               "source_sha": HEAD_SHA, "observation_session_id": SESSION,
               "unit_ledger_digest": "b3-" + "cd" * 32}
    binding.update(overrides)
    return binding


def _final_report():
    return {"run_key": RUN_KEY, "report_id": f"final-{RUN_KEY}",
            "status": "passed", "counts": {}}


def _final_report_for(attempt):
    run_key = f"r{RUN_ID}-a{attempt}"
    return {"run_key": run_key, "report_id": f"final-{run_key}",
            "status": "passed", "counts": {}}


def _write_packet(root, members):
    run_path = root / "run.json"
    jobs_path = root / "jobs.json"
    zip_path = root / "final.zip"
    artifact_path = root / "artifact.json"
    run_path.write_text(json.dumps(_run_doc()), encoding="utf-8")
    jobs_path.write_text(json.dumps(_jobs_doc()), encoding="utf-8")
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, payload in members.items():
            archive.writestr(name, payload)
    payload = zip_path.read_bytes()
    artifact_path.write_text(json.dumps({
        "total_count": 1,
        "artifacts": [{"id": ARTIFACT_ID,
                       "name": f"velnor-final-{RUN_KEY}",
                       "url": f"https://api.github.com/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT_ID}",
                       "size_in_bytes": len(payload),
                       "digest": f"sha256:{hashlib.sha256(payload).hexdigest()}",
                       "workflow_run": {"id": RUN_ID, "head_sha": HEAD_SHA}}]}),
        encoding="utf-8")
    return run_path, jobs_path, zip_path, artifact_path


def _verified_collection(members=None):
    root = Path(tempfile.mkdtemp(prefix="gc1-observation-"))
    if members is None:
        members = {"final-report.json": json.dumps(_final_report()),
                   MEMBER: json.dumps(_binding())}
    run_path, jobs_path, zip_path, artifact_path = _write_packet(root, members)
    run = S.load_json(run_path)
    jobs = S.validate_identity(run, S.load_json(jobs_path), REPOSITORY)
    effective = E.build_effective_job_map(jobs)
    S.validate_artifact(artifact_path, ARTIFACT_ID, zip_path, run, REPOSITORY)
    with zipfile.ZipFile(zip_path) as archive:
        collected = S.collect_bound_observations(
            archive, run_key=RUN_KEY, head_sha=HEAD_SHA, effective_jobs=effective)
    return collected


HELPER_JOB = "helper-job"
REPLAY_JOB = "replay-preparation-job"
BOOTSTRAP_JOB = "bootstrap-job"
HELPER_ID = 103
REPLAY_ID = 105
BOOTSTRAP_ID = 104
RERUN_JOB_ID = 102


def _task_id(attempt, matrix_suffix, report_suffix):
    return f"task-r{RUN_ID}-a{attempt}-m-{matrix_suffix}-{report_suffix}"


def _obs_member(matrix_key, task_report_id):
    return f"{matrix_key}/tasks/{task_report_id}{S.OBSERVATION_SUFFIX}"


def _job_record(name, numeric_id, attempt, conclusion="success"):
    return {"id": numeric_id, "run_id": RUN_ID, "run_attempt": attempt,
            "head_sha": HEAD_SHA, "name": name,
            "run_url": f"https://api.github.com/repos/{REPOSITORY}/actions/runs/{RUN_ID}",
            "html_url": f"https://github.com/{REPOSITORY}/actions/runs/{RUN_ID}/job/{numeric_id}",
            "status": "completed", "conclusion": conclusion}


def _binding_for(run_key, job_name, attempt, task_report_id, member):
    return {"schema": 1, "task_report_id": task_report_id,
            "artifact_id": f"velnor-crate-{run_key}-{job_name}",
            "member_name": member, "member_size_bytes": 512,
            "member_digest": "b3-" + "ab" * 32,
            "producer_job": job_name, "producer_attempt": attempt,
            "source_sha": HEAD_SHA, "observation_session_id": SESSION,
            "unit_ledger_digest": "b3-" + "cd" * 32}


def _verified_attempt(attempt, job_records, members):
    """Verify one attempt packet end to end and return (jobs, collected)."""
    root = Path(tempfile.mkdtemp(prefix="gc1-lifecycle-"))
    run_key = f"r{RUN_ID}-a{attempt}"
    run_path = root / "run.json"
    jobs_path = root / "jobs.json"
    zip_path = root / "final.zip"
    artifact_path = root / "artifact.json"
    run_path.write_text(json.dumps({"id": RUN_ID, "run_attempt": attempt, "head_sha": HEAD_SHA}),
                        encoding="utf-8")
    jobs_path.write_text(json.dumps({"total_count": len(job_records), "jobs": job_records}),
                         encoding="utf-8")
    members = dict(members)
    members.setdefault("final-report.json", json.dumps(_final_report_for(attempt)))
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, payload in members.items():
            archive.writestr(name, payload)
    payload = zip_path.read_bytes()
    artifact_path.write_text(json.dumps({
        "total_count": 1,
        "artifacts": [{"id": ARTIFACT_ID,
                       "name": f"velnor-final-{run_key}",
                       "url": f"https://api.github.com/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT_ID}",
                       "size_in_bytes": len(payload),
                       "digest": f"sha256:{hashlib.sha256(payload).hexdigest()}",
                       "workflow_run": {"id": RUN_ID, "head_sha": HEAD_SHA}}]}),
        encoding="utf-8")
    run = S.load_json(run_path)
    jobs = S.validate_identity(run, S.load_json(jobs_path), REPOSITORY)
    effective = E.build_effective_job_map(jobs)
    S.validate_artifact(artifact_path, ARTIFACT_ID, zip_path, run, REPOSITORY)
    with zipfile.ZipFile(zip_path) as archive:
        collected = S.collect_bound_observations(
            archive, run_key=run_key, head_sha=HEAD_SHA, effective_jobs=effective)
    return jobs, collected


class Gc1ObservationTests(unittest.TestCase):
    def test_accepts_bound_observation(self):
        collected = _verified_collection()
        self.assertEqual(collected["status"], "verified")
        self.assertEqual(collected["observation_count"], 1)
        record = collected["accepted"][TASK_REPORT_ID]
        self.assertEqual(record["member_digest"], "b3-" + "ab" * 32)
        self.assertEqual(record["effective_job_id"], JOB_ID)
        self.assertEqual(record["producer_attempt"], ATTEMPT)
        self.assertEqual(record["source_sha"], HEAD_SHA)
        self.assertEqual(S.admit_measured_counter(7, task_report_id=TASK_REPORT_ID,
                                                  accepted=collected["accepted"]), 7)
        self.assertEqual(S.admit_measured_counter(0, task_report_id=TASK_REPORT_ID,
                                                  accepted=collected["accepted"]), 0)

    def test_rejects_unbound_zero(self):
        with self.assertRaises(ValueError):
            S.admit_measured_counter(0, task_report_id=TASK_REPORT_ID, accepted={})
        with self.assertRaises(ValueError):
            S.admit_measured_counter(None, task_report_id=TASK_REPORT_ID, accepted={})
        collected = _verified_collection()
        with self.assertRaises(ValueError):
            S.admit_measured_counter(None, task_report_id=TASK_REPORT_ID,
                                     accepted=collected["accepted"])

    def test_rejects_forged_effective_attempt(self):
        forged = {"final-report.json": json.dumps(_final_report()),
                  MEMBER: json.dumps(_binding(producer_attempt=ATTEMPT + 1))}
        with self.assertRaises(ValueError):
            _verified_collection(forged)

    def test_rejects_cross_source_binding(self):
        forged = {"final-report.json": json.dumps(_final_report()),
                  MEMBER: json.dumps(_binding(source_sha=OTHER_SHA))}
        with self.assertRaises(ValueError):
            _verified_collection(forged)

    def test_rejects_unindexed_member(self):
        stray = "m-aaaaaaaaaaaaaaaa/tasks/ghost.observation.json"
        members = {"final-report.json": json.dumps(_final_report()),
                   stray: json.dumps(_binding(member_name=stray))}
        with self.assertRaises(ValueError):
            _verified_collection(members)

    def test_missing_observation_stays_unknown(self):
        task_member = f"{MATRIX_KEY}/tasks/{TASK_REPORT_ID}.json"
        members = {"final-report.json": json.dumps(_final_report()),
                   task_member: json.dumps({"task_report_id": TASK_REPORT_ID})}
        collected = _verified_collection(members)
        self.assertEqual(collected["observation_count"], 0)
        self.assertEqual(len(collected["missing_task_report_observations"]), 1)
        missing = collected["missing_task_report_observations"][0]
        self.assertEqual(missing["disposition"], "unknown")
        self.assertTrue(missing["unknown_reason"])
        with self.assertRaises(ValueError):
            S.admit_measured_counter(0, task_report_id=TASK_REPORT_ID,
                                     accepted=collected["accepted"])

    def test_join_uses_effective_attempt(self):
        attempt = 2
        run_key = f"r{RUN_ID}-a{attempt}"
        matrix = f"m-{'11' * 8}"
        rerun_task = _task_id(attempt, "11" * 8, "22" * 8)
        helper_task = _task_id(attempt, "33" * 8, "44" * 8)
        jobs = [_job_record(JOB_NAME, RERUN_JOB_ID, attempt, conclusion="failure"),
                _job_record(HELPER_JOB, HELPER_ID, attempt)]
        members = {_obs_member(matrix, rerun_task): json.dumps(
                       _binding_for(run_key, JOB_NAME, attempt, rerun_task,
                                    _obs_member(matrix, rerun_task))),
                   _obs_member(matrix, helper_task): json.dumps(
                       _binding_for(run_key, HELPER_JOB, attempt, helper_task,
                                    _obs_member(matrix, helper_task)))}
        jobs, collected = _verified_attempt(attempt, jobs, members)
        effective = E.build_effective_job_map(jobs)
        lifecycle = C.build_lifecycle_aggregates(
            repository=REPOSITORY, run_id=RUN_ID, head_sha=HEAD_SHA,
            jobs=jobs, effective_jobs=effective, collected=collected)
        by_task = {row["task_report_id"]: row for row in lifecycle["join"]["joined"]}
        # The join resolves each observation to the authenticated record for
        # its actual attempt: the rerun keeps its new job id, it is not
        # relabelled to the earlier attempt's record.
        self.assertEqual(by_task[rerun_task]["effective_job_id"], RERUN_JOB_ID)
        self.assertEqual(by_task[rerun_task]["producer_attempt"], attempt)
        self.assertEqual(by_task[helper_task]["effective_job_id"], HELPER_ID)
        self.assertEqual(lifecycle["join"]["observation_count"], 2)
        # A forged attempt with no authenticated record fails instead of joining.
        forged_task = _task_id(attempt, "55" * 8, "66" * 8)
        forged = {_obs_member(matrix, forged_task): json.dumps(
                      _binding_for(run_key, JOB_NAME, attempt + 1, forged_task,
                                   _obs_member(matrix, forged_task)))}
        with self.assertRaises(ValueError):
            _verified_attempt(attempt, jobs, forged)
        # A stale attempt GitHub no longer retains fails the lifecycle join too.
        stale = dict(collected["accepted"])
        stale_record = dict(stale[rerun_task])
        stale_record["producer_attempt"] = attempt - 1
        stale[rerun_task] = stale_record
        stale_collected = {"accepted": stale,
                           "missing_task_report_observations": []}
        with self.assertRaises(ValueError):
            C.build_lifecycle_aggregates(
                repository=REPOSITORY, run_id=RUN_ID, head_sha=HEAD_SHA,
                jobs=jobs, effective_jobs=effective, collected=stale_collected)

    def test_pipeline_includes_helpers_and_replay(self):
        run_key_1 = f"r{RUN_ID}-a1"
        run_key_2 = f"r{RUN_ID}-a2"
        matrix_1 = f"m-{'aa' * 8}"
        matrix_2 = f"m-{'bb' * 8}"
        retained_task = _task_id(1, "aa" * 8, "bb" * 8)
        uncovered_task = _task_id(1, "cc" * 8, "dd" * 8)
        rerun_task = _task_id(2, "ee" * 8, "ff" * 8)
        helper_task = _task_id(2, "11" * 8, "22" * 8)
        replay_task = _task_id(2, "33" * 8, "44" * 8)
        jobs_1 = [_job_record(JOB_NAME, JOB_ID, 1),
                  _job_record(BOOTSTRAP_JOB, BOOTSTRAP_ID, 1)]
        members_1 = {_obs_member(matrix_1, retained_task): json.dumps(
                         _binding_for(run_key_1, JOB_NAME, 1, retained_task,
                                      _obs_member(matrix_1, retained_task))),
                     f"{matrix_1}/tasks/{uncovered_task}.json": json.dumps(
                         {"task_report_id": uncovered_task})}
        jobs_2 = [_job_record(JOB_NAME, RERUN_JOB_ID, 2, conclusion="failure"),
                  _job_record(HELPER_JOB, HELPER_ID, 2),
                  _job_record(REPLAY_JOB, REPLAY_ID, 2)]
        members_2 = {}
        for task, producer in ((rerun_task, JOB_NAME), (helper_task, HELPER_JOB),
                               (replay_task, REPLAY_JOB)):
            members_2[_obs_member(matrix_2, task)] = json.dumps(
                _binding_for(run_key_2, producer, 2, task, _obs_member(matrix_2, task)))
        jobs_1, collected_1 = _verified_attempt(1, jobs_1, members_1)
        jobs_2, collected_2 = _verified_attempt(2, jobs_2, members_2)
        jobs = jobs_1 + jobs_2
        effective = E.build_effective_job_map(jobs)
        collected = {"accepted": {**collected_1["accepted"], **collected_2["accepted"]},
                     "missing_task_report_observations": (
                         collected_1["missing_task_report_observations"]
                         + collected_2["missing_task_report_observations"])}
        lifecycle = C.build_lifecycle_aggregates(
            repository=REPOSITORY, run_id=RUN_ID, head_sha=HEAD_SHA,
            jobs=jobs, effective_jobs=effective, collected=collected)
        executed = lifecycle["executed_lifecycle"]
        # Every real job is counted once: bootstrap, helpers, preparation,
        # replay, and the failed rerun attempt are all included.
        self.assertEqual(executed["job_count"], 5)
        self.assertEqual(len({row["job_id"] for row in executed["jobs"]}), 5)
        self.assertEqual({row["job_name"] for row in executed["jobs"]},
                         {JOB_NAME, BOOTSTRAP_JOB, HELPER_JOB, REPLAY_JOB})
        self.assertEqual(executed["by_conclusion"].get("failure"), 1)
        # Effective Required results cover the retained job once and the
        # rerun's new work in addition, each exactly once.
        covered = {(row["job_id"], row["actual_attempt"])
                   for row in lifecycle["effective_required"]["covered_job_attempts"]}
        self.assertEqual(covered, {(JOB_ID, 1), (RERUN_JOB_ID, 2),
                                   (HELPER_ID, 2), (REPLAY_ID, 2)})
        self.assertEqual(lifecycle["effective_required"]["covered_job_attempt_count"], 4)
        self.assertEqual(lifecycle["effective_required"]["bound_task_report_count"], 4)
        # Jobs and task reports without a bound observation stay unknown, never zero.
        uncovered = lifecycle["missing_coverage"]["jobs_without_bound_observation"]
        self.assertEqual([row["job_id"] for row in uncovered], [BOOTSTRAP_ID])
        self.assertEqual(uncovered[0]["disposition"], "unknown")
        self.assertTrue(uncovered[0]["unknown_reason"])
        missing_tasks = lifecycle["missing_coverage"]["task_reports_without_observation"]
        self.assertEqual(len(missing_tasks), 1)
        self.assertEqual(missing_tasks[0]["task_report_id"], uncovered_task)
        self.assertEqual(lifecycle["missing_coverage"]["disposition"], "unknown")
        # Unobserved process work stays null with a reason; no zero compiler counts.
        processes = lifecycle["process_counts"]
        self.assertIsNone(processes["compiler_executions"])
        self.assertTrue(processes["compiler_executions_unknown_reason"])
        self.assertIsNone(processes["native_process_executions"])
        self.assertTrue(processes["native_process_executions_unknown_reason"])
        # Origin bytes, cache bytes, and historical MBX summaries stay separate.
        channels = lifecycle["byte_channels"]
        self.assertIsNone(channels["origin_acquisition_bytes"])
        self.assertIsNone(channels["cache_transfer_bytes"])
        self.assertIsNone(channels["mbx_object_cache_summaries"])
        self.assertTrue(channels["origin_acquisition_bytes_unknown_reason"])
        self.assertTrue(channels["cache_transfer_bytes_unknown_reason"])
        self.assertTrue(channels["mbx_object_cache_summaries_unknown_reason"])
        self.assertIn("never summed together", channels["separation_rule"])


RENAMED_DISPLAY = "Unit Job Build"
RENAMED_ID = 201
CONTROL_TASK = f"task-{RUN_KEY}-m-cccccccccccccccc-dddddddddddddddd"
WF_PATH = "workflows/unit.yml"
WF_TEXT = "name: hosted-measurement\non: push\njobs:\n  unit-job:\n    name: unit-job\n    runs-on: ubuntu-latest\n"


def _verified_graph(nodes):
    return {"status": "verified", "nodes": nodes,
            "sha256": "0" * 64, "parser_scope": "test"}


def _verified_attempt_with_graph(attempt, job_records, members, graph):
    root = Path(tempfile.mkdtemp(prefix="gc1-f6-"))
    run_key = f"r{RUN_ID}-a{attempt}"
    run_path = root / "run.json"
    jobs_path = root / "jobs.json"
    zip_path = root / "final.zip"
    artifact_path = root / "artifact.json"
    run_path.write_text(json.dumps({"id": RUN_ID, "run_attempt": attempt, "head_sha": HEAD_SHA}),
                        encoding="utf-8")
    jobs_path.write_text(json.dumps({"total_count": len(job_records), "jobs": job_records}),
                         encoding="utf-8")
    members = dict(members)
    members.setdefault("final-report.json", json.dumps(_final_report_for(attempt)))
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, payload in members.items():
            archive.writestr(name, payload)
    payload = zip_path.read_bytes()
    artifact_path.write_text(json.dumps({
        "total_count": 1,
        "artifacts": [{"id": ARTIFACT_ID,
                       "name": f"velnor-final-{run_key}",
                       "url": f"https://api.github.com/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT_ID}",
                       "size_in_bytes": len(payload),
                       "digest": f"sha256:{hashlib.sha256(payload).hexdigest()}",
                       "workflow_run": {"id": RUN_ID, "head_sha": HEAD_SHA}}]}),
        encoding="utf-8")
    run = S.load_json(run_path)
    jobs = S.validate_identity(run, S.load_json(jobs_path), REPOSITORY)
    effective = E.build_effective_job_map(jobs, workflow_graph=graph)
    S.validate_artifact(artifact_path, ARTIFACT_ID, zip_path, run, REPOSITORY)
    with zipfile.ZipFile(zip_path) as archive:
        collected = S.collect_bound_observations(
            archive, run_key=run_key, head_sha=HEAD_SHA, effective_jobs=effective)
    return jobs, collected


def _write_control_packet(root):
    payload = WF_TEXT.encode("utf-8")
    blob = hashlib.sha1(b"blob " + str(len(payload)).encode() + b"\0" + payload).hexdigest()
    run = {"id": RUN_ID, "run_attempt": ATTEMPT, "head_sha": HEAD_SHA,
           "path": WF_PATH, "event": "push", "workflow_id": 9,
           "status": "completed", "conclusion": "success",
           "created_at": "2026-03-04T10:00:00Z"}
    job = {"id": JOB_ID, "run_id": RUN_ID, "run_attempt": ATTEMPT,
           "head_sha": HEAD_SHA, "name": JOB_NAME,
           "run_url": f"https://api.github.com/repos/{REPOSITORY}/actions/runs/{RUN_ID}",
           "html_url": f"https://github.com/{REPOSITORY}/actions/runs/{RUN_ID}/job/{JOB_ID}",
           "status": "completed", "conclusion": "success",
           "started_at": "2026-03-04T10:01:00Z", "completed_at": "2026-03-04T10:05:00Z",
           "steps": [{"number": 1, "name": "Build", "status": "completed",
                      "conclusion": "success",
                      "started_at": "2026-03-04T10:01:10Z",
                      "completed_at": "2026-03-04T10:04:50Z"}]}
    run_path = root / "run.json"
    jobs_path = root / "jobs.json"
    logs_path = root / "logs.zip"
    workflow_json_path = root / "workflow.json"
    workflow_path = root / "unit.yml"
    final_path = root / "final.zip"
    artifact_path = root / "artifact.json"
    task_dir = root / "task-reports" / f"velnor-crate-{RUN_KEY}-{JOB_NAME}" / "tasks"
    task_dir.mkdir(parents=True)
    run_path.write_text(json.dumps(run), encoding="utf-8")
    jobs_path.write_text(json.dumps({"total_count": 1, "jobs": [job]}), encoding="utf-8")
    with zipfile.ZipFile(logs_path, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("unit-job/1_Build.txt", "2026-03-04T10:01:10Z Build ok\n")
    workflow_json_path.write_text(json.dumps({
        "path": WF_PATH,
        "url": f"https://api.github.com/repos/{REPOSITORY}/contents/{WF_PATH}?ref={HEAD_SHA}",
        "encoding": "base64", "content": base64.b64encode(payload).decode("ascii"),
        "sha": blob}), encoding="utf-8")
    workflow_path.write_bytes(payload)
    with zipfile.ZipFile(final_path, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("final-report.json", json.dumps(_final_report()))
        archive.writestr(MEMBER, json.dumps(_binding()))
        archive.writestr(f"{MATRIX_KEY}/tasks/{CONTROL_TASK}.json",
                         json.dumps({"task_report_id": CONTROL_TASK}))
    archive_payload = final_path.read_bytes()
    artifact_path.write_text(json.dumps({
        "total_count": 2,
        "artifacts": [{"id": ARTIFACT_ID,
                       "name": f"velnor-final-{RUN_KEY}",
                       "url": f"https://api.github.com/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT_ID}",
                       "size_in_bytes": len(archive_payload),
                       "digest": f"sha256:{hashlib.sha256(archive_payload).hexdigest()}",
                       "workflow_run": {"id": RUN_ID, "head_sha": HEAD_SHA}},
                      {"id": ARTIFACT_ID + 1,
                       "name": f"velnor-crate-{RUN_KEY}-{JOB_NAME}",
                       "url": f"https://api.github.com/repos/{REPOSITORY}/actions/artifacts/{ARTIFACT_ID + 1}",
                       "workflow_run": {"id": RUN_ID, "head_sha": HEAD_SHA}}]}),
        encoding="utf-8")
    (task_dir / f"{CONTROL_TASK}.json").write_text(json.dumps(
        {"run_key": RUN_KEY, "timing": {field: None for field in S.STAGE_TIMINGS}}),
        encoding="utf-8")
    out_path = root / "report.json"
    return {"run": run_path, "jobs": jobs_path, "logs": logs_path,
            "workflow_json": workflow_json_path, "workflow": workflow_path,
            "artifact": artifact_path, "final": final_path,
            "task_dir": root / "task-reports", "out": out_path}


class F6JoinTests(unittest.TestCase):
    def test_matching_control_cli_exit_zero(self):
        root = Path(tempfile.mkdtemp(prefix="gc1-f6-control-"))
        paths = _write_control_packet(root)
        argv = ["measure-hosted-run", "--repository", REPOSITORY,
                "--run-json", str(paths["run"]), "--jobs-json", str(paths["jobs"]),
                "--logs-zip", str(paths["logs"]),
                "--workflow-json", str(paths["workflow_json"]),
                "--workflow-file", str(paths["workflow"]),
                "--artifact-json", str(paths["artifact"]),
                "--artifact-id", str(ARTIFACT_ID),
                "--final-artifact", str(paths["final"]),
                "--task-report-dir", str(paths["task_dir"]),
                "--out", str(paths["out"])]
        with mock.patch.object(sys, "argv", argv):
            self.assertEqual(R.cli(), 0)
        report = json.loads(paths["out"].read_text(encoding="utf-8"))
        lifecycle = report["observed_lifecycle"]
        self.assertEqual(lifecycle["status"], "verified")
        self.assertEqual(lifecycle["join"]["key_basis"], "workflow-source-job-id")
        self.assertEqual(lifecycle["join"]["observation_count"], 1)
        envelope = lifecycle["observation_envelope"]
        self.assertEqual(envelope["observation_count"], 1)
        self.assertEqual(envelope["accepted"][0]["task_report_id"], TASK_REPORT_ID)
        self.assertEqual(envelope["accepted"][0]["effective_job_id"], JOB_ID)
        self.assertEqual(envelope["accepted"][0]["member_digest"], "b3-" + "ab" * 32)
        missing = lifecycle["missing_coverage"]["task_reports_without_observation"]
        self.assertEqual(len(missing), 1)
        self.assertEqual(missing[0]["task_report_id"], CONTROL_TASK)
        self.assertEqual(missing[0]["disposition"], "unknown")

    def test_renamed_api_display_name_joins_via_workflow_graph(self):
        attempt = 1
        run_key = f"r{RUN_ID}-a{attempt}"
        matrix = f"m-{'12' * 8}"
        task = _task_id(attempt, "12" * 8, "34" * 8)
        graph = _verified_graph([{"job_id": JOB_NAME, "display_name": RENAMED_DISPLAY,
                                  "needs": []}])
        jobs = [_job_record(RENAMED_DISPLAY, RENAMED_ID, attempt)]
        members = {_obs_member(matrix, task): json.dumps(
            _binding_for(run_key, JOB_NAME, attempt, task, _obs_member(matrix, task)))}
        jobs, collected = _verified_attempt_with_graph(attempt, jobs, members, graph)
        self.assertEqual(collected["observation_count"], 1)
        record = collected["accepted"][task]
        self.assertEqual(record["effective_job_id"], RENAMED_ID)
        self.assertEqual(record["producer_attempt"], attempt)
        lifecycle = C.build_lifecycle_aggregates(
            repository=REPOSITORY, run_id=RUN_ID, head_sha=HEAD_SHA,
            jobs=jobs, effective_jobs=E.build_effective_job_map(jobs, workflow_graph=graph),
            collected=collected, key_basis="workflow-source-job-id")
        self.assertEqual(lifecycle["join"]["joined"][0]["effective_job_id"], RENAMED_ID)
        self.assertEqual(lifecycle["join"]["key_basis"], "workflow-source-job-id")

    def test_retained_earlier_attempt_accepted_with_attempts_preserved(self):
        attempt = 2
        run_key = f"r{RUN_ID}-a{attempt}"
        matrix = f"m-{'56' * 8}"
        rerun_task = _task_id(attempt, "56" * 8, "78" * 8)
        jobs = [_job_record(JOB_NAME, JOB_ID, 1),
                _job_record(JOB_NAME, RERUN_JOB_ID, attempt, conclusion="failure"),
                _job_record(HELPER_JOB, HELPER_ID, attempt)]
        members = {_obs_member(matrix, rerun_task): json.dumps(
            _binding_for(run_key, JOB_NAME, attempt, rerun_task,
                         _obs_member(matrix, rerun_task)))}
        jobs, collected = _verified_attempt(attempt, jobs, members)
        attempts = sorted(job["run_attempt"] for job in jobs)
        self.assertEqual(attempts, [1, 2, 2])
        self.assertEqual(collected["observation_count"], 1)
        lifecycle = C.build_lifecycle_aggregates(
            repository=REPOSITORY, run_id=RUN_ID, head_sha=HEAD_SHA,
            jobs=jobs, effective_jobs=E.build_effective_job_map(jobs),
            collected=collected)
        by_task = {row["task_report_id"]: row for row in lifecycle["join"]["joined"]}
        self.assertEqual(by_task[rerun_task]["effective_job_id"], RERUN_JOB_ID)
        self.assertEqual(by_task[rerun_task]["producer_attempt"], attempt)
        executed = {row["job_id"]: row["run_attempt"]
                    for row in lifecycle["executed_lifecycle"]["jobs"]}
        self.assertEqual(executed, {JOB_ID: 1, RERUN_JOB_ID: 2, HELPER_ID: 2})
        uncovered = lifecycle["missing_coverage"]["jobs_without_bound_observation"]
        self.assertEqual({row["job_id"] for row in uncovered}, {JOB_ID, HELPER_ID})
        for row in uncovered:
            self.assertEqual(row["disposition"], "unknown")
            self.assertTrue(row["unknown_reason"])

    def test_display_name_claim_without_source_key_rejected(self):
        attempt = 1
        graph = _verified_graph([{"job_id": "build", "display_name": JOB_NAME,
                                  "needs": []}])
        jobs = [_job_record(JOB_NAME, RENAMED_ID, attempt)]
        effective = E.build_effective_job_map(jobs, workflow_graph=graph)
        self.assertEqual(list(effective), [("build", attempt)])
        with self.assertRaises(ValueError):
            E.resolve_effective_job(effective, producer_job=JOB_NAME,
                                    producer_attempt=attempt)
        with self.assertRaises(ValueError):
            S.parse_observation_binding(json.dumps(_binding(producer_job=RENAMED_DISPLAY)),
                                        MEMBER)

    def test_ambiguous_log_folder_rejected(self):
        jobs = [_job_record(JOB_NAME, JOB_ID, ATTEMPT),
                _job_record(JOB_NAME, RERUN_JOB_ID, ATTEMPT)]
        with self.assertRaises(ValueError):
            E.collect_job_events(jobs, {})


if __name__ == "__main__":
    raise SystemExit(unittest.main())
