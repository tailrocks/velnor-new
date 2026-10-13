#!/usr/bin/env python3
"""Read-only, attempt-bound Actions evidence collection into a private directory."""

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys


# Official GitHub REST OpenAPI components.schemas.job.properties.conclusion.
# https://github.com/github/rest-api-description/blob/main/descriptions/api.github.com/api.github.com.json
JOB_CONCLUSIONS = frozenset({"success", "failure", "neutral", "cancelled", "skipped",
                             "timed_out", "action_required"})
ACQUIRED_API_FILES = frozenset({"run.json", "jobs.json", "artifacts.json"})


def api(endpoint, paginate=False):
    """Keep raw responses unfiltered; never print API bodies or credentials."""
    command = ["rtk", "proxy", "gh", "api", endpoint, "--allow-escape-sequences"]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    result = subprocess.run(command, capture_output=True, check=False)
    if result.returncode:
        # stderr can include sensitive URLs. Retain only the HTTP status.
        match = re.search(rb"HTTP (\d{3})", result.stderr)
        status = match.group(1).decode() if match else "unknown"
        raise RuntimeError(f"API unavailable (HTTP {status})")
    return result.stdout


def write(path, content):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK,
                         0o600)
    with os.fdopen(descriptor, "wb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError("evidence destination must be a regular file with one link")
        os.fchmod(stream.fileno(), 0o600)
        stream.truncate(0)
        stream.write(content)


def read(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError("evidence source must be a regular file with one link")
        return stream.read()


def json_api(endpoint, destination, paginate=False, reuse=False, acquisition=None,
             expected_digest=None):
    if reuse:
        return json.loads(verify_acquired_digest(destination, expected_digest))
    content = api(endpoint, paginate)
    parsed = json.loads(content)
    write(destination, content)
    if acquisition is not None:
        acquisition[destination.name] = hashlib.sha256(content).hexdigest()
    return parsed


def seconds(start, end):
    if not start or not end:
        return None
    first = dt.datetime.fromisoformat(start.replace("Z", "+00:00"))
    last = dt.datetime.fromisoformat(end.replace("Z", "+00:00"))
    value = (last - first).total_seconds()
    return value if value >= 0 else None


def log_observations(content):
    """Extract only pinned-tool summary grammar; don't infer process work."""
    result = {"mbx_sessions": [], "archive_observations": []}
    text = content.decode("utf-8", errors="replace")
    grammar = re.compile(
        r"mbx\[cache\]: object cache: (\d+) hits, (\d+) misses"
        r"(?:, (\d+) not looked up)?(?:, (\d+) bypassed)?;"
    )
    for line_number, line in enumerate(text.splitlines(), 1):
        found = grammar.search(line)
        if found:
            hits, misses, not_looked, bypassed = found.groups()
            result["mbx_sessions"].append({
                "line": line_number, "hits": int(hits), "misses": int(misses),
                "not_looked_up": int(not_looked) if not_looked else None,
                "bypassed": int(bypassed) if bypassed else None,
                "interpretation": "object summaries; not compiler-process counts",
            })
        archive = re.search(r"Cache Size: ~[^\n]*\((\d+) B\)", line)
        if archive:
            result["archive_observations"].append({
                "line": line_number, "compressed_bytes": int(archive.group(1)),
            })
    return result


def collect_job(repository, job, directory, reuse, original_logs):
    raw = directory / f"job-{job['id']}.log"
    log = {"available": False, "sha256": None, "error": None}
    observations = {"mbx_sessions": [], "archive_observations": []}
    try:
        original = original_logs.get(str(job["id"])) if reuse else None
        if reuse and not original["available"]:
            raise RuntimeError(original["error"] or "Original log unavailable")
        content = read(raw) if reuse else api(
            f"repos/{repository}/actions/jobs/{job['id']}/logs"
        )
        if reuse and hashlib.sha256(content).hexdigest() != original["sha256"]:
            raise ValueError("saved log differs from original acquisition digest")
        if not reuse:
            write(raw, content)
        log.update(available=True, sha256=hashlib.sha256(content).hexdigest())
        observations = log_observations(content)
    except RuntimeError as error:
        log["error"] = str(error)
    steps = [{
        "number": step["number"], "name": step["name"],
        "conclusion": step.get("conclusion"),
        "started_at": step.get("started_at"),
        "completed_at": step.get("completed_at"),
        "wall_seconds": seconds(step.get("started_at"), step.get("completed_at")),
    } for step in job.get("steps", [])]
    return {
        "id": job["id"], "url": job["html_url"], "name": job["name"],
        "conclusion": job.get("conclusion"), "run_attempt": job.get("run_attempt"),
        "runner_id": job.get("runner_id"), "runner_name": job.get("runner_name"),
        "labels": job.get("labels"), "steps": steps,
        "started_at": job.get("started_at"), "completed_at": job.get("completed_at"),
        "wall_seconds": seconds(job.get("started_at"), job.get("completed_at")),
        "raw_log": log, **observations,
        "compiler_process_seconds": None, "link_seconds": None,
        "build_script_seconds": None, "rustdoc_seconds": None,
        "tool_payload_download_bytes": None, "source_download_bytes": None,
        "cargo_fresh_units": None, "test_execution_seconds": None,
        "cache_payload_versions": None, "useful_export_delta": None,
    }


def validate_directory(directory):
    root = Path(__file__).resolve().parents[1]
    if directory == root or root in directory.parents:
        raise ValueError("raw evidence directory must be outside the repository")
    if any((parent / ".git").exists() for parent in (directory, *directory.parents)):
        raise ValueError("raw evidence directory must be outside every Git checkout")
    directory.mkdir(parents=True, exist_ok=True)
    directory.chmod(0o700)


def complete_pages(pages, field):
    if not pages:
        raise ValueError(f"missing {field} pages")
    totals = {page.get("total_count") for page in pages}
    rows = [row for page in pages for row in page[field]]
    if len(totals) != 1 or None in totals or len(rows) != next(iter(totals)):
        raise ValueError(f"incomplete or inconsistent {field} pagination")
    if len({row["id"] for row in rows}) != len(rows):
        raise ValueError(f"duplicate {field} IDs")
    return rows


def positive_integer(value):
    if type(value) is not int or value <= 0:
        raise ValueError("identity must be a positive integer")
    return value


def admitted_jobs(pages, identity):
    if not isinstance(pages, list) or not pages:
        raise ValueError("missing jobs pages")
    for page in pages:
        if not isinstance(page, dict) or type(page.get("total_count")) is not int:
            raise ValueError("jobs page/count malformed")
        if page["total_count"] < 0 or not isinstance(page.get("jobs"), list):
            raise ValueError("jobs page/count malformed")
        for job in page["jobs"]:
            if not isinstance(job, dict):
                raise ValueError("job must be a mapping")
            positive_integer(job["id"])
    jobs = complete_pages(pages, "jobs")
    for job in jobs:
        if (positive_integer(job["run_id"]) != identity[1]
                or positive_integer(job["run_attempt"]) != identity[2]
                or job.get("head_sha") != identity[3]):
            raise ValueError("API job identity mismatch")
        conclusion = job.get("conclusion")
        if (job.get("status") != "completed" or not isinstance(conclusion, str)
                or conclusion not in JOB_CONCLUSIONS):
            raise ValueError("job must have completed status and terminal conclusion")
        if not isinstance(job.get("name"), str):
            raise ValueError("job name must be text")
    return jobs


def verify_acquired_digest(path, checksum):
    if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise ValueError("original acquisition digest malformed")
    content = read(path)
    if hashlib.sha256(content).hexdigest() != checksum:
        raise ValueError("saved evidence differs from original acquisition digest")
    return content


def original_acquisition_manifest(arguments, directory):
    """Reuse checks original local acquisition digests; never rewrite that record."""
    value = json.loads(read(directory / "original-acquisition.json"))
    if (not isinstance(value, dict) or type(value.get("schema")) is not int or value["schema"] != 1
            or value.get("repository") != arguments.repository
            or value.get("run_id") != arguments.run or value.get("attempt") != arguments.attempt
            or not isinstance(value.get("logs"), dict)):
        raise ValueError("original log acquisition identity malformed")
    positive_integer(value["run_id"])
    positive_integer(value["attempt"])
    source = value.get("source_commit")
    if not isinstance(source, str) or not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("original log acquisition source malformed")
    responses = value.get("api_responses")
    if not isinstance(responses, dict) or set(responses) != ACQUIRED_API_FILES:
        raise ValueError("original API acquisition inventory malformed")
    for filename, checksum in responses.items():
        verify_acquired_digest(directory / filename, checksum)
    for identifier, record in value["logs"].items():
        if (not identifier.isdecimal() or str(int(identifier)) != identifier
                or int(identifier) <= 0 or not isinstance(record, dict)):
            raise ValueError("original log acquisition record malformed")
        if type(record.get("available")) is not bool:
            raise ValueError("original log acquisition availability malformed")
        if record.get("error") is not None and not isinstance(record["error"], str):
            raise ValueError("original log acquisition error malformed")
        if not record["available"] and (directory / f"job-{identifier}.log").exists():
            raise ValueError("unacquired log unexpectedly present")
        if record["available"]:
            verify_acquired_digest(directory / f"job-{identifier}.log", record.get("sha256"))
    return value


def retain_original_acquisition(arguments, directory, jobs, source, responses):
    value = {"schema": 1, "repository": arguments.repository, "run_id": arguments.run,
             "attempt": arguments.attempt, "source_commit": source,
             "api_responses": responses,
             "logs": {str(job["id"]): job["raw_log"] for job in jobs}}
    if set(responses) != ACQUIRED_API_FILES:
        raise ValueError("acquired API inventory incomplete")
    for filename, checksum in responses.items():
        verify_acquired_digest(directory / filename, checksum)
    descriptor = os.open(directory / "original-acquisition.json",
                         os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(json.dumps(value, indent=2).encode() + b"\n")


def collect(arguments):
    directory = arguments.output.expanduser().resolve()
    validate_directory(directory)
    if not arguments.reuse and any(directory.iterdir()):
        raise ValueError("new collection requires an unused evidence directory")
    original_manifest = original_acquisition_manifest(arguments, directory) if arguments.reuse else {}
    original_logs = original_manifest.get("logs", {})
    acquired_api = {}
    expected_api = original_manifest.get("api_responses", {})
    prefix = f"repos/{arguments.repository}/actions/runs/{arguments.run}"
    run = json_api(f"{prefix}/attempts/{arguments.attempt}",
                   directory / "run.json", reuse=arguments.reuse, acquisition=acquired_api,
                   expected_digest=expected_api.get("run.json"))
    if (positive_integer(run.get("id")) != arguments.run
            or run.get("repository", {}).get("full_name") != arguments.repository):
        raise ValueError("saved/API run identity differs from requested repository/run")
    if arguments.reuse and original_manifest.get("source_commit") != run.get("head_sha"):
        raise ValueError("saved source differs from original log acquisition")
    source = run.get("head_sha")
    if not isinstance(source, str) or not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("source SHA malformed")
    if positive_integer(run.get("run_attempt")) != arguments.attempt:
        raise ValueError("API attempt differs from requested attempt")
    if run.get("status") != "completed":
        raise ValueError("run must be completed before immutable evidence collection")
    pages = json_api(f"{prefix}/attempts/{arguments.attempt}/jobs?per_page=100",
                     directory / "jobs.json", paginate=True, reuse=arguments.reuse,
                     acquisition=acquired_api, expected_digest=expected_api.get("jobs.json"))
    jobs = admitted_jobs(pages, (arguments.repository, arguments.run,
                                 arguments.attempt, run["head_sha"]))
    if arguments.reuse and set(original_logs) != {str(job["id"]) for job in jobs}:
        raise ValueError("saved job inventory differs from original log acquisition")
    artifacts = json_api(f"{prefix}/artifacts?per_page=100",
                          directory / "artifacts.json", paginate=True,
                          reuse=arguments.reuse, acquisition=acquired_api,
                          expected_digest=expected_api.get("artifacts.json"))
    artifact_rows = complete_pages(artifacts, "artifacts")
    if any(artifact.get("workflow_run", {}).get("id") != arguments.run
           or artifact["workflow_run"].get("head_sha") != run["head_sha"]
           or artifact["workflow_run"].get("repository_id") != run["repository"]["id"]
           for artifact in artifact_rows):
        raise ValueError("artifact workflow identity differs from requested source/run")
    details = [collect_job(arguments.repository, job, directory, arguments.reuse, original_logs)
               for job in jobs]
    active = [job for job in jobs if job.get("steps")]
    starts = [job["started_at"] for job in active if job.get("started_at")]
    ends = [job["completed_at"] for job in active if job.get("completed_at")]
    summary = {
        "schema": 1, "repository": arguments.repository,
        "summary_created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "reused_existing_evidence": arguments.reuse,
        "run_id": arguments.run, "attempt": arguments.attempt,
        "source_commit": run["head_sha"], "event": run["event"],
        "url": run["html_url"], "conclusion": run.get("conclusion"),
        "workflow_id": run["workflow_id"], "workflow_path": run.get("path"),
        "created_at": run["created_at"], "run_started_at": run.get("run_started_at"),
        "original_run_creation_to_first_job_start_seconds": seconds(run["created_at"], min(starts))
        if starts else None,
        "pure_queue_seconds": None, "runner_provision_seconds": None,
        "observed_job_envelope_seconds": seconds(min(starts), max(ends))
        if starts and ends else None,
        "sum_job_wall_seconds": sum(job["wall_seconds"] for job in details)
        if details and all(job["wall_seconds"] is not None for job in details) else None,
        "jobs": details, "artifact_count": len(artifact_rows),
        "limitations": [
            "Creation-to-first-job interval uses original run.created_at; reruns include intervening elapsed time, not attempt queue/provision time.",
            "Job envelope is observed wall time; dependency critical path needs the plan.",
            "Step API times have one-second resolution and include command overhead.",
            "Regex log counters are untrusted observations, not authenticated owning-tool/session telemetry.",
            "MBX counters exclude Cargo reuse and GitHub archive transfers.",
            "No compiler-work inference from Compiling lines or zero misses.",
            "Artifact listing is run-wide; qualify each artifact's attempt separately.",
            "Null metrics require supported telemetry or manual raw-evidence review.",
            "This collection does not establish successful performance qualification.",
        ],
    }
    if not arguments.reuse:
        retain_original_acquisition(arguments, directory, details, run["head_sha"], acquired_api)
    write(directory / "summary.json", json.dumps(summary, indent=2).encode() + b"\n")
    print(f"Collected {len(jobs)} jobs; private summary: {directory / 'summary.json'}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repository", help="exact owner/repository from scope.json")
    parser.add_argument("run", type=int)
    parser.add_argument("--attempt", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--reuse", action="store_true", help="reuse saved immutable responses")
    args = parser.parse_args()
    scope = json.loads((Path(__file__).resolve().parents[1] / "scope.json").read_text())
    if args.repository not in {row["repository"] for row in scope["repositories"]}:
        parser.error("repository absent from exact task scope")
    if args.run <= 0 or args.attempt <= 0:
        parser.error("run and attempt must be positive")
    os.umask(0o077)
    try:
        collect(args)
    except (RuntimeError, ValueError, OSError, KeyError) as error:
        print(f"Collection failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
