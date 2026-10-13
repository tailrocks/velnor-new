#!/usr/bin/env python3
"""Recompute attempt timelines from collector evidence and source-verified DAGs."""

import argparse
import base64
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys

import yaml

if yaml.__version__ != "6.0.3":
    raise RuntimeError("install the pinned CI analysis requirements in an isolated environment")


def load_collector():
    location = Path(__file__).with_name("collect-ci-performance.py")
    spec = importlib.util.spec_from_file_location("collector", location)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


COLLECTOR = load_collector()
# Official GitHub REST OpenAPI components.schemas.job.properties.conclusion.
# https://github.com/github/rest-api-description/blob/main/descriptions/api.github.com/api.github.com.json
JOB_CONCLUSIONS = frozenset({"success", "failure", "neutral", "cancelled", "skipped",
                             "timed_out", "action_required"})


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate document key")
        result[key] = value
    return result


class StrictYaml(yaml.SafeLoader):
    yaml_implicit_resolvers = {
        key: [(tag, pattern) for tag, pattern in entries
              if tag != "tag:yaml.org,2002:bool"]
        for key, entries in yaml.SafeLoader.yaml_implicit_resolvers.items()
    }


def yaml_mapping(loader, node):
    loader.flatten_mapping(node)
    return unique_pairs(loader.construct_pairs(node))


StrictYaml.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG,
                          yaml_mapping)
StrictYaml.add_implicit_resolver("tag:yaml.org,2002:bool",
                                re.compile(r"^(?:true|false)$", re.IGNORECASE),
                                list("tTfF"))


def document(path):
    return json.loads(COLLECTOR.read(path), object_pairs_hook=unique_pairs)


def timestamp(value):
    if not isinstance(value, str):
        raise ValueError("timestamp must be present text")
    parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("timestamp missing timezone")
    return parsed


def positive_integer(value):
    if type(value) is not int or value <= 0:
        raise ValueError("identity must be a positive integer")
    return value


class ExecutedWorkflowRevisionUnavailable(ValueError):
    pass


def require_push_source(event):
    if event != "push":
        raise ExecutedWorkflowRevisionUnavailable("unsupported event source resolution")


def evidence_identity(summary, run):
    if not isinstance(summary, dict) or not isinstance(run, dict):
        raise ValueError("summary and run must be mappings")
    if type(summary.get("schema")) is not int or summary["schema"] != 1:
        raise ValueError("unsupported collector summary schema")
    repository = summary["repository"]
    if not isinstance(repository, str) or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("repository identity malformed")
    source = summary["source_commit"]
    if not isinstance(source, str) or not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("source SHA malformed")
    expected = (repository, positive_integer(summary["run_id"]),
                positive_integer(summary["attempt"]), source)
    run_repository = run.get("repository")
    if not isinstance(run_repository, dict):
        raise ValueError("run repository must be a mapping")
    observed = (run_repository["full_name"], positive_integer(run["id"]),
                positive_integer(run["run_attempt"]), run["head_sha"])
    if expected != observed or run.get("status") != "completed":
        raise ValueError("summary and run-attempt identities differ")
    if summary.get("event") != run.get("event"):
        raise ValueError("summary and run event identities differ")
    require_push_source(run.get("event"))
    return expected


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
    jobs = COLLECTOR.complete_pages(pages, "jobs")
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


def workflow_for(run, repository):
    require_push_source(run.get("event"))
    path = run["path"].split("@", 1)[0]
    sha = run["head_sha"]
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("source SHA malformed")
    if not re.fullmatch(r"\.github/workflows/[A-Za-z0-9_.-]+\.ya?ml", path):
        raise ValueError("workflow path malformed")
    response = json.loads(COLLECTOR.api(
        f"repos/{repository}/contents/{path}?ref={sha}"),
        object_pairs_hook=unique_pairs)
    if response.get("path") != path or response.get("encoding") != "base64":
        raise ValueError("workflow source response mismatch")
    content = base64.b64decode(response["content"])
    if hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest() != response["sha"]:
        raise ValueError("workflow Git blob digest mismatch")
    if len(content) > 1024 * 1024:
        raise ValueError("workflow exceeds analysis bound")
    workflow = yaml.load(content, Loader=StrictYaml)
    return workflow, content, path, response["sha"]


def graph(workflow, jobs):
    if not isinstance(workflow, dict) or not isinstance(workflow.get("jobs"), dict):
        raise ValueError("workflow/jobs must be mappings")
    definitions = workflow["jobs"]
    by_name = {}
    for identifier, definition in definitions.items():
        if not isinstance(identifier, str) or not isinstance(definition, dict):
            raise ValueError("job definition must be a named mapping")
        if "uses" in definition:
            raise ValueError("reusable workflow expansion needs a qualified mapping; unsupported")
        name = definition.get("name", identifier)
        if not isinstance(name, str) or "${{" in name or definition.get("strategy"):
            raise ValueError("dynamic job expansion needs a qualified mapping; unsupported")
        if name in by_name:
            raise ValueError("duplicate workflow job display name")
        by_name[name] = identifier
    actual = {}
    for job in jobs:
        if job["name"] not in by_name:
            raise ValueError("API job does not map to source workflow")
        identifier = by_name[job["name"]]
        if identifier in actual:
            raise ValueError("multiple API jobs map to one static job")
        actual[identifier] = job
    if actual.keys() != definitions.keys():
        raise ValueError("incomplete API jobs versus workflow definitions")
    dependencies = {}
    for identifier, definition in definitions.items():
        needs = definition.get("needs", [])
        needs = [needs] if isinstance(needs, str) else needs
        if not isinstance(needs, list) or any(item not in actual for item in needs):
            raise ValueError("unknown or malformed workflow dependency")
        if len(needs) != len(set(needs)):
            raise ValueError("duplicate dependency")
        dependencies[identifier] = needs
    return actual, dependencies


def analyze(jobs, dependencies):
    active = {key: value for key, value in jobs.items()
              if value.get("conclusion") != "skipped"}
    if not active:
        raise ValueError("no executed jobs to measure")
    origin = min(timestamp(job["started_at"]) for job in active.values())
    states, paths, weighted = {}, {}, {}

    def visit(identifier):
        if states.get(identifier) == "visiting":
            raise ValueError("workflow dependency cycle")
        if identifier in paths:
            return
        states[identifier] = "visiting"
        for parent in dependencies[identifier]:
            visit(parent)
        job = jobs[identifier]
        predecessors = []
        pending = list(dependencies[identifier])
        seen = set()
        while pending:
            parent = pending.pop()
            if parent in seen:
                continue
            seen.add(parent)
            if parent in active:
                predecessors.append(parent)
            else:
                pending.extend(dependencies[parent])
        if identifier not in active:
            paths[identifier] = []
            weighted[identifier] = 0
        else:
            start, end = timestamp(job["started_at"]), timestamp(job["completed_at"])
            wall = (end - start).total_seconds()
            if wall < 0:
                raise ValueError("negative job wall interval")
            parent = max(predecessors, key=lambda key: timestamp(jobs[key]["completed_at"]),
                         default=None)
            eligible = timestamp(jobs[parent]["completed_at"]) if parent else origin
            gap = (start - eligible).total_seconds()
            if gap < -1:
                raise ValueError("job started before required predecessor completed")
            paths[identifier] = (paths[parent] if parent else []) + [{
                "workflow_job": identifier, "job_id": job["id"],
                "wall_seconds": wall,
                "dependency_start_delay_seconds": max(gap, 0),
                "timestamp_overlap_seconds": max(-gap, 0),
                "delay_interpretation": "scheduling/provisioning not separately measured",
            }]
            weighted[identifier] = wall + max((weighted[key] for key in predecessors), default=0)
        states[identifier] = "visited"

    for identifier in jobs:
        visit(identifier)
    terminal = max(active, key=lambda key: timestamp(jobs[key]["completed_at"]))
    path = paths[terminal]
    return {
        "observed_completion_path": path,
        "completion_path_seconds": sum(row["wall_seconds"] + row["dependency_start_delay_seconds"] - row["timestamp_overlap_seconds"]
                                       for row in path),
        "api_timestamp_resolution_seconds": 1,
        "longest_job_wall_dependency_path_seconds": max(weighted.values()),
        "runner_wall_sum_seconds": sum((timestamp(job["completed_at"]) - timestamp(job["started_at"])).total_seconds()
                                       for job in active.values()),
        "pure_queue_seconds": None, "runner_provision_seconds": None,
        "compiler_process_seconds": None, "link_seconds": None,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    directory = args.evidence.expanduser().resolve()
    try:
        COLLECTOR.validate_directory(directory)
        summary, run = document(directory / "summary.json"), document(directory / "run.json")
        expected = evidence_identity(summary, run)
        scope = document(Path(__file__).resolve().parents[1] / "scope.json")
        if expected[0] not in {row["repository"] for row in scope["repositories"]}:
            raise ValueError("repository outside exact scope")
        jobs = admitted_jobs(document(directory / "jobs.json"), expected)
        workflow, content, path, blob = workflow_for(run, expected[0])
        actual, dependencies = graph(workflow, jobs)
        result = {"schema": 1, "repository": expected[0], "run_id": expected[1],
                  "attempt": expected[2], "source_commit": expected[3],
                  "event": run["event"], "workflow_source_resolution": "push_head_sha",
                  "workflow_path": path, "workflow_git_blob": blob,
                  "workflow_sha256": hashlib.sha256(content).hexdigest(),
                  "dag_origin": "push head source contents API; static jobs/needs",
                  **analyze(actual, dependencies)}
        COLLECTOR.write(directory / "workflow-source.yml", content)
        COLLECTOR.write(directory / "timeline-analysis.json", json.dumps(result, indent=2).encode() + b"\n")
        print("Source-bound timeline analysis saved in private evidence directory")
    except (ValueError, KeyError, TypeError, OSError, RuntimeError, yaml.YAMLError) as error:
        print(f"Timeline unavailable: {type(error).__name__}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
