import json
import hashlib
import sys
import tempfile
import unittest
import zipfile
from argparse import Namespace
from pathlib import Path

PACKAGE_PARENT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PACKAGE_PARENT))
from hosted_phase_measurement import measure_hosted_run as measure  # noqa: E402


REPO = "tailrocks/velnor-new"
SHA = "a" * 40


def job(job_id, name, start, end, steps, *, attempt=1, head=SHA, created="2026-10-04T00:00:00Z"):
    return {
        "id": job_id,
        "run_id": 123,
        "run_attempt": attempt,
        "head_sha": head,
        "name": name,
        "run_url": f"https://api.github.com/repos/{REPO}/actions/runs/123",
        "html_url": f"https://github.com/{REPO}/actions/runs/123/job/{job_id}",
        "created_at": created,
        "started_at": start,
        "completed_at": end,
        "status": "completed",
        "conclusion": "success",
        "labels": ["ubuntu-26.04"],
        "runner_name": "GitHub Actions 1",
        "runner_id": 1,
        "steps": steps,
    }


def step(number, name, start, end, status="completed", conclusion="success"):
    return {"number": number, "name": name, "started_at": start, "completed_at": end, "status": status, "conclusion": conclusion}


def fixture_report():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        run = {"id": 123, "run_attempt": 1, "head_sha": SHA, "event": "push", "workflow_id": 5, "path": ".github/workflows/ci.yml", "status": "completed", "conclusion": "success", "created_at": "2026-10-04T00:00:00Z"}
        steps_a = [step(1, "Prepare pinned tools", "2026-10-04T00:00:01Z", "2026-10-04T00:00:11Z"), step(2, "Restore Cargo sources", "2026-10-04T00:00:10Z", "2026-10-04T00:00:11Z")]
        steps_b = [step(1, "Unit and integration tests", "2026-10-04T00:00:06Z", "2026-10-04T00:00:16Z")]
        jobs = [
            job(1, "Plan", "2026-10-04T00:00:01Z", "2026-10-04T00:00:11Z", steps_a),
            job(2, "Rust / crate", "2026-10-04T00:00:06Z", "2026-10-04T00:00:16Z", steps_b),
        ]
        with zipfile.ZipFile(root / "logs.zip", "w") as archive:
            archive.writestr("Plan/1_Prepare pinned tools.txt", "2026-10-04T00:00:02Z Installing rust\n")
            archive.writestr("Plan/2_Restore Cargo sources.txt", "2026-10-04T00:00:10Z Received 20 of 20 (100%), 10 MBs/sec\n")
            archive.writestr("Rust _ crate/1_Unit and integration tests.txt", "test result: ok\n")
            archive.writestr("Plan/system.txt", "job summary, not an API step\n")
            archive.writestr("summary.txt", "archive summary\n")
            archive.writestr("Plan/step/attempt.txt", "nested non-step candidate\n")
        paths = []
        for name, doc in (("run.json", run), ("jobs.json", {"total_count": 2, "jobs": jobs})):
            path = root / name
            path.write_text(json.dumps(doc))
            paths.append(path)
        args = Namespace(repository=REPO, run_json=paths[0], jobs_json=paths[1], logs_zip=root / "logs.zip", workflow_json=None, workflow_file=None, artifact_json=None, artifact_id=None, final_artifact=None, task_report_dir=None)
        return measure.build_report(args)


class MeasurementTests(unittest.TestCase):
    def test_job_attempt_and_source_are_mandatory(self):
        run = {"id": 123, "run_attempt": 1, "head_sha": SHA}
        wrong = job(1, "Plan", "2026-10-04T00:00:01Z", "2026-10-04T00:00:02Z", [], attempt=2)
        with self.assertRaisesRegex(ValueError, "different run/attempt/source"):
            measure.validate_identity(run, {"total_count": 1, "jobs": [wrong]}, REPO)

    def test_api_step_span_is_not_queue_and_critical_path_does_not_sum_workers(self):
        report = fixture_report()
        self.assertEqual(report["timeline"]["observed_job_span_envelope_ms"], 15_000)
        self.assertIsNone(report["timeline"]["workflow_critical_path_ms"])
        self.assertEqual(report["timeline"]["workflow_critical_path_status"], "unavailable")
        self.assertEqual(report["phase_worker_time"]["tool_provision"]["cumulative_job_step_ms"], 10_000)
        self.assertEqual(report["phase_worker_time"]["unit_integration_tests"]["cumulative_job_step_ms"], 10_000)
        totals = report["actual_counters"]["cache_transfer_totals_by_phase"]["cargo_source_restore"]
        self.assertEqual(totals["total_bytes"], 20)
        self.assertEqual(totals["enclosing_api_step_span_ms_sum"], 1_000)
        self.assertEqual(report["schema"], "velnor.hosted-phase-measurement.v4")
        logs = report["source_receipts"]["logs_archive"]
        self.assertEqual(logs["zip_entry_count_total"], 6)
        self.assertEqual(logs["selected_two_part_txt_candidate_count"], 4)
        self.assertEqual(logs["joined_api_step_log_count"], 3)
        self.assertNotIn("entry_count", logs)
        self.assertIn({"api_step_name": "Restore Cargo sources", "normalized_step_name": "restore cargo sources", "phase": "cargo_source_restore", "api_step_count": 1}, report["phase_map"]["observed_step_name_map"])
        self.assertIsNone(report["timeline"]["runner_queue_duration_ms"])
        self.assertIsNone(report["jobs"][0]["queue_duration_ms"])
        self.assertEqual(report["jobs"][0]["created_to_started_ms_unclassified"], 1000)

    def test_report_unknown_counters_have_null_values_and_reasons(self):
        report = fixture_report()
        expected_reasons = {
            "tool_download_duration_ms": ("tool_download_duration_unknown_reason", "Mise log lines contain progress snapshots, not a unique completed transfer interval; enclosing Prepare pinned tools API step span is reported by phase."),
            "tool_download_bytes": ("tool_download_bytes_unknown_reason", "Pinned Mise progress output has human-readable progress, not a stable authoritative byte counter."),
            "cargo_download_bytes": ("cargo_download_bytes_unknown_reason", "Cargo log output does not provide authoritative per-package transferred byte counts."),
            "link_duration_ms": ("link_duration_unknown_reason", "No separate linker start/end event is present; Cargo build/test step spans combine compiler and link work."),
            "fresh_compiler_units": ("fresh_compiler_units_unknown_reason", "The retained run has no complete compiler-unit receipt; Compiling lines and MBX lookup counters are not unit totals."),
            "lock_wait_ms": ("lock_wait_unknown_reason", "Hosted APIs and retained logs contain no separate lock-wait measurement; task report zero sentinels are unavailable telemetry."),
            "runner_queue_duration_ms": ("runner_queue_duration_unknown_reason", "The retained workflow-run and jobs APIs expose no queue-start/end pair or dependency-wait split."),
            "native_cpu": ("native_cpu_unknown_reason", "Local compiler observer receipts lack exact hosted run/job/attempt/source identity and are not joinable to this run."),
        }
        actual = report["actual_counters"]
        for counter, (reason_field, expected_reason) in expected_reasons.items():
            with self.subTest(counter=counter):
                self.assertIsNone(actual[counter])
                self.assertEqual(actual[reason_field], expected_reason)

    def test_unknown_counter_zero_mutation_is_rejected_for_each_report_field(self):
        actual = fixture_report()["actual_counters"]
        for counter in measure.UNKNOWN_ACTUAL_COUNTER_REASON_FIELDS:
            with self.subTest(counter=counter):
                mutant = dict(actual)
                mutant[counter] = 0
                with self.assertRaisesRegex(ValueError, rf"actual_counters\.{counter} must be null"):
                    measure.validate_actual_counter_unknowns(mutant)

    def test_cumulative_cache_bytes_are_not_added_and_transfer_time_stays_unknown(self):
        raw = ("2026-10-04T00:00:01Z Received 10 of 20 (50.0%), 10 MBs/sec\n"
               "2026-10-04T00:00:02Z Received 20 of 20 (100.0%), 10 MBs/sec\n"
               "2026-10-04T00:00:03Z Cache Size: ~1 MB (20 B)\n").encode()
        evidence = measure.extract_log_evidence(raw, "cargo_source_restore", "Plan/1_Restore Cargo sources.txt")
        self.assertEqual(evidence["cache_bytes_received"], 20)
        self.assertIsNone(evidence["cache_transfer_duration_ms"])
        self.assertEqual([row["received_bytes"] for row in evidence["cache_received_observations"]], [10, 20])
        uploaded = measure.extract_log_evidence(b"2026-10-04T00:00:01Z Sent 10 of 20 (50%), 10 MBs/sec\n2026-10-04T00:00:02Z Sent 20 of 20 (100%), 10 MBs/sec\n", "mbx_bundle_save", "Plan/1_Save MBX bundle.txt")
        self.assertEqual(uploaded["cache_bytes_uploaded"], 20)
        self.assertEqual([row["uploaded_bytes"] for row in uploaded["cache_sent_observations"]], [10, 20])

    def test_actual_step49_maps_post_restore_to_distinct_mbx_upload(self):
        raw = (Path(__file__).parent / "fixtures" / "runner-core-step-49-post-restore-mbx-objects.txt").read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), "2221dcfd7e75c097876fdb634f0572b5ef36f20f4e061db055c59a34a1b0adf9")
        phase = measure.phase_for_step("Post Restore MBX objects")
        self.assertEqual(phase, "mbx_object_post_upload")
        self.assertEqual(measure.phase_for_step("Restore MBX objects"), "mbx_object_restore")
        self.assertEqual(measure.phase_for_step("prefix Post Restore MBX objects suffix"), "other")
        evidence = measure.extract_log_evidence(raw, phase, "Rust _ velnor-runner-core/49_Post Restore MBX objects.txt")
        self.assertEqual([row["uploaded_bytes"] for row in evidence["cache_sent_observations"]], [23_855_104, 29_062_963])
        self.assertEqual(evidence["cache_bytes_uploaded"], 29_062_963)
        self.assertNotIn("cache_bytes_received", evidence)
        self.assertIsNone(evidence["cache_transfer_duration_ms"])

    def test_compiling_text_is_not_compiler_units_or_link_time(self):
        evidence = measure.extract_log_evidence(b"Compiling example v1.0.0\nLinking example\n", "test_build", "job/1_build.txt")
        self.assertEqual(evidence["mbx_object_cache_summaries"], [])
        self.assertEqual(measure.phase_for_step("Build test executables"), "test_build")
        self.assertNotIn("compiler_units", evidence)

    def test_mbx_summary_remains_scoped_to_one_cache_observation(self):
        raw = b"2026-10-04T00:00:01Z mbx[cache]: object cache: 4 hits, 1 miss, 3 not looked up, 2 bypassed; 0 B downloaded, 12 B uploaded, 3.5 MiB stored locally\n"
        evidence = measure.extract_log_evidence(raw, "clippy", "job/1_Clippy.txt")
        row = evidence["mbx_object_cache_summaries"][0]
        self.assertEqual(row["counts"], {"hits": 4, "misses": 1, "not_looked_up": 3, "bypassed": 2})
        self.assertIn("not measure total compiler freshness", row["counter_scope"])

    def test_tool_progress_does_not_become_bytes_or_completed_download(self):
        raw = b"2026-10-04T00:00:01Z rust@1.98.1 Downloading rustup-init 3.0s 30/30 kB\n"
        row = measure.extract_log_evidence(raw, "tool_provision", "job/1_prepare.txt")["mise_download_progress"][0]
        self.assertEqual(row["reported_progress"], "30/30 kB")
        self.assertFalse(row["completion_proven"])
        self.assertNotIn("bytes", row)

    def test_task_stage_zero_sentinels_are_unavailable_not_measured(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for index in range(71):
                row = {"run_key": "r123-a1", "timing": {field: (None if field == "link_ms" else 0) for field in measure.STAGE_TIMINGS}}
                (root / f"task-{index}.json").write_text(json.dumps(row))
            result = measure.inspect_task_reports(root, 123, 1, SHA, None)
        self.assertEqual(result["report_count"], 71)
        self.assertTrue(result["all_stage_values_zero_or_null"])
        self.assertEqual(result["stage_field_value_counts"]["link_ms"]["null"], 71)
        self.assertEqual(result["status"], "unavailable_as_phase_measurement")

    def test_task_reports_from_another_attempt_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "task-report.json").write_text(json.dumps({"run_key": "r123-a2", "timing": {}}))
            with self.assertRaisesRegex(ValueError, "not bound to the requested run attempt"):
                measure.inspect_task_reports(root, 123, 1, SHA, None)

    def test_task_report_parent_artifact_name_must_bind_attempt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_name = "velnor-crate-r123-a2-crate"
            artifact_dir = root / artifact_name
            artifact_dir.mkdir()
            (artifact_dir / "task-1.json").write_text(json.dumps({"run_key": "r123-a1", "timing": {}}))
            artifact_doc = {"artifacts": [{"name": artifact_name, "workflow_run": {"id": 123, "head_sha": SHA}}]}
            with self.assertRaisesRegex(ValueError, "requested run attempt and source"):
                measure.inspect_task_reports(root, 123, 1, SHA, artifact_doc)

    def test_workflow_api_blob_is_bound_to_exact_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = b"name: CI\njobs:\n  build:\n    steps:\n      - uses: actions/cache/restore@" + b"a" * 40 + b"\n"
            blob = __import__("hashlib").sha1(b"blob " + str(len(payload)).encode() + b"\0" + payload).hexdigest()
            encoded = __import__("base64").b64encode(payload).decode()
            api = root / "contents.json"
            api.write_text(json.dumps({"path": ".github/workflows/ci.yml", "sha": blob, "encoding": "base64", "content": encoded, "url": f"https://api.github.com/repos/{REPO}/contents/.github/workflows/ci.yml?ref={SHA}"}))
            workflow = root / "ci.yml"
            workflow.write_bytes(payload)
            run = {"path": ".github/workflows/ci.yml", "head_sha": SHA}
            result = measure.workflow_receipt(api, workflow, run, REPO)
            self.assertEqual(result["status"], "verified")
            self.assertTrue(result["workflow_action_refs"][0]["full_sha_pinned"])
            workflow.write_bytes(payload + b"# changed\n")
            with self.assertRaisesRegex(ValueError, "differ from contents API"):
                measure.workflow_receipt(api, workflow, run, REPO)

    def test_workflow_graph_requires_static_names_and_needs(self):
        payload = b"jobs:\n  plan:\n    name: Plan\n    steps: []\n  crate:\n    name: Crate\n    needs: plan\n    steps: []\n  required:\n    name: Required\n    needs: [plan, crate]\n    steps: []\n"
        graph = measure.parse_workflow_job_graph(payload)
        self.assertEqual(graph["status"], "verified")
        self.assertEqual(graph["nodes"][2]["needs"], ["plan", "crate"])
        dynamic = measure.parse_workflow_job_graph(b"jobs:\n  build:\n    name: Build ${{ matrix.os }}\n")
        self.assertEqual(dynamic["status"], "unavailable")
        self.assertIn("dynamic display name", dynamic["reason"])

    def test_workflow_dag_path_uses_needs_and_exact_job_step_log_joins(self):
        graph = measure.parse_workflow_job_graph(b"jobs:\n  plan:\n    name: Plan\n  crate:\n    name: Crate\n    needs: plan\n  required:\n    name: Required\n    needs: crate\n")
        jobs = [
            {"id": 10, "name": "Plan", "conclusion": "success", "started_at": "2026-10-04T00:00:00Z", "completed_at": "2026-10-04T00:00:10Z"},
            {"id": 11, "name": "Crate", "conclusion": "success", "started_at": "2026-10-04T00:00:11Z", "completed_at": "2026-10-04T00:00:20Z"},
            {"id": 12, "name": "Required", "conclusion": "success", "started_at": "2026-10-04T00:00:22Z", "completed_at": "2026-10-04T00:00:25Z"},
        ]
        steps = [
            {"job_id": row["id"], "step_number": 1, "status": "completed", "conclusion": "success", "started_at": row["started_at"], "completed_at": row["completed_at"], "duration_ms_from_api_timestamps": measure.duration_ms(row["started_at"], row["completed_at"], "fixture"), "log_evidence": {"log_sha256": str(row["id"])}}
            for row in jobs
        ]
        result = measure.compute_workflow_dag_path(graph, jobs, steps, [])
        self.assertEqual(result["status"], "verified")
        self.assertEqual(result["source_job_ids"], ["plan", "crate", "required"])
        self.assertEqual(result["api_job_ids"], [10, 11, 12])
        self.assertEqual(result["duration_ms"], 25_000)
        self.assertEqual(result["unclassified_post_dependency_ready_wait_ms"], 3_000)
        self.assertIsNone(result["queue_duration_ms"])
        skipped = {"job_id": 11, "step_number": 2, "status": "completed", "conclusion": "skipped", "started_at": None, "completed_at": None, "duration_ms_from_api_timestamps": None, "log_evidence": None}
        with_skipped = measure.compute_workflow_dag_path(graph, jobs, steps + [skipped], [])
        self.assertEqual(with_skipped["path_api_step_count"], 4)
        self.assertEqual(with_skipped["joined_api_step_count"], 3)
        self.assertEqual(with_skipped["skipped_api_step_count"], 1)
        missing = measure.compute_workflow_dag_path(graph, jobs, steps, [{"job_id": 11}])
        self.assertIsNone(missing["duration_ms"])
        self.assertIn("exact API step identity", missing["reason"])
        before_dependency = [dict(row) for row in jobs]
        before_dependency[2]["started_at"] = "2026-10-04T00:00:19Z"
        invalid = measure.compute_workflow_dag_path(graph, before_dependency, steps, [])
        self.assertIsNone(invalid["duration_ms"])
        self.assertIn("before its latest dependency", invalid["reason"])

    def test_dag_path_is_not_the_overall_job_span_envelope(self):
        graph = measure.parse_workflow_job_graph(b"jobs:\n  early:\n    name: Early\n  late:\n    name: Late\n  join:\n    name: Join\n    needs: [early, late]\n")
        jobs = [
            {"id": 20, "name": "Early", "conclusion": "success", "started_at": "2026-10-04T00:00:00Z", "completed_at": "2026-10-04T00:00:09Z"},
            {"id": 21, "name": "Late", "conclusion": "success", "started_at": "2026-10-04T00:00:08Z", "completed_at": "2026-10-04T00:00:10Z"},
            {"id": 22, "name": "Join", "conclusion": "success", "started_at": "2026-10-04T00:00:20Z", "completed_at": "2026-10-04T00:00:21Z"},
        ]
        steps = [
            {"job_id": row["id"], "step_number": 1, "status": "completed", "conclusion": "success", "started_at": row["started_at"], "completed_at": row["completed_at"], "duration_ms_from_api_timestamps": measure.duration_ms(row["started_at"], row["completed_at"], "fixture"), "log_evidence": {"log_sha256": str(row["id"])}}
            for row in jobs
        ]
        result = measure.compute_workflow_dag_path(graph, jobs, steps, [])
        self.assertEqual(result["source_job_ids"], ["late", "join"])
        self.assertEqual(result["duration_ms"], 13_000)
        self.assertEqual(result["unclassified_post_dependency_ready_wait_ms"], 10_000)
        self.assertNotEqual(result["duration_ms"], 21_000)

    def test_dag_equal_completion_uses_longest_path_and_ignores_needs_order(self):
        jobs = [
            {"id": 30, "name": "Long root", "conclusion": "success", "started_at": "2026-10-04T00:00:00Z", "completed_at": "2026-10-04T00:00:50Z"},
            {"id": 31, "name": "Long tail", "conclusion": "success", "started_at": "2026-10-04T00:00:50Z", "completed_at": "2026-10-04T00:01:00Z"},
            {"id": 32, "name": "Short", "conclusion": "success", "started_at": "2026-10-04T00:00:55Z", "completed_at": "2026-10-04T00:01:00Z"},
            {"id": 33, "name": "Join", "conclusion": "success", "started_at": "2026-10-04T00:01:00Z", "completed_at": "2026-10-04T00:01:10Z"},
        ]
        steps = [
            {"job_id": row["id"], "step_number": 1, "status": "completed", "conclusion": "success", "started_at": row["started_at"], "completed_at": row["completed_at"], "duration_ms_from_api_timestamps": measure.duration_ms(row["started_at"], row["completed_at"], "tie fixture"), "log_evidence": {"log_sha256": str(row["id"])} }
            for row in jobs
        ]
        prefix = b"jobs:\n  root:\n    name: Long root\n  tail:\n    name: Long tail\n    needs: root\n  short:\n    name: Short\n"
        suffix = b"  join:\n    name: Join\n    needs: "
        results = []
        for needs in (b"[short, tail]", b"[tail, short]"):
            graph = measure.parse_workflow_job_graph(prefix + suffix + needs + b"\n")
            results.append(measure.compute_workflow_dag_path(graph, jobs, steps, []))
        self.assertEqual([row["duration_ms"] for row in results], [70_000, 70_000])
        self.assertEqual([row["source_job_ids"] for row in results], [["root", "tail", "join"]] * 2)
        self.assertEqual([row["critical_predecessor_by_source_job"]["join"] for row in results], ["tail", "tail"])
        for row in results:
            offsets = row["dependency_completion_offsets_ms_by_source_job"]["join"]
            self.assertTrue(offsets["short"]["latest_completed_parent"])
            self.assertTrue(offsets["tail"]["latest_completed_parent"])
            self.assertTrue(offsets["tail"]["selected_critical_path_predecessor"])
            self.assertFalse(offsets["short"]["selected_critical_path_predecessor"])

    def test_dag_equal_accumulated_tie_uses_stable_source_id(self):
        jobs = [
            {"id": 40, "name": "Zulu", "conclusion": "success", "started_at": "2026-10-04T00:00:00Z", "completed_at": "2026-10-04T00:00:10Z"},
            {"id": 41, "name": "Alpha", "conclusion": "success", "started_at": "2026-10-04T00:00:00Z", "completed_at": "2026-10-04T00:00:10Z"},
            {"id": 42, "name": "Join", "conclusion": "success", "started_at": "2026-10-04T00:00:10Z", "completed_at": "2026-10-04T00:00:20Z"},
        ]
        steps = [
            {"job_id": row["id"], "step_number": 1, "status": "completed", "conclusion": "success", "started_at": row["started_at"], "completed_at": row["completed_at"], "duration_ms_from_api_timestamps": measure.duration_ms(row["started_at"], row["completed_at"], "tie fixture"), "log_evidence": {"log_sha256": str(row["id"])} }
            for row in jobs
        ]
        graphs = [
            measure.parse_workflow_job_graph(b"jobs:\n  zulu:\n    name: Zulu\n  alpha:\n    name: Alpha\n  join:\n    name: Join\n    needs: [zulu, alpha]\n"),
            measure.parse_workflow_job_graph(b"jobs:\n  zulu:\n    name: Zulu\n  alpha:\n    name: Alpha\n  join:\n    name: Join\n    needs: [alpha, zulu]\n"),
        ]
        results = [measure.compute_workflow_dag_path(graph, jobs, steps, []) for graph in graphs]
        self.assertEqual([row["source_job_ids"] for row in results], [["alpha", "join"]] * 2)
        self.assertEqual([row["duration_ms"] for row in results], [20_000, 20_000])

    def test_final_artifact_digest_and_attempt_are_api_bound(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = json.dumps({"run_key": "r123-a1", "report_id": "final-r123-a1", "status": "passed", "counts": {"executed": 1}}).encode()
            archive_path = root / "artifact.zip"
            with zipfile.ZipFile(archive_path, "w") as archive:
                archive.writestr("final-report.json", report)
            payload = archive_path.read_bytes()
            manifest_path = root / "artifacts.json"
            row = {"id": 77, "name": "velnor-final-r123-a1", "url": f"https://api.github.com/repos/{REPO}/actions/artifacts/77", "size_in_bytes": len(payload), "digest": f"sha256:{hashlib.sha256(payload).hexdigest()}", "workflow_run": {"id": 123, "head_sha": SHA}}
            manifest_path.write_text(json.dumps({"total_count": 1, "artifacts": [row]}))
            run = {"id": 123, "run_attempt": 1, "head_sha": SHA}
            _, result = measure.validate_artifact(manifest_path, 77, archive_path, run, REPO)
            self.assertEqual(result["report_status"], "passed")
            row["workflow_run"]["head_sha"] = "b" * 40
            manifest_path.write_text(json.dumps({"total_count": 1, "artifacts": [row]}))
            with self.assertRaisesRegex(ValueError, "does not bind name, repository, run, attempt, and source SHA"):
                measure.validate_artifact(manifest_path, 77, archive_path, run, REPO)


if __name__ == "__main__":
    unittest.main()
