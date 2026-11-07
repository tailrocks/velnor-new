# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys

MAX_PLAN = 4 << 20
CLI = "rust-velnor-actions-cli"


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def digest(value):
    require(re.fullmatch(r"[0-9a-f]{64}", value) is not None, "invalid digest")
    return value


def regular(path, limit):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= limit,
            "unsafe input")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
    require(len(data) == info.st_size and len(data) <= limit
            and (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
            == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns), "changed input")
    return data


def parse_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def select():
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    if event_name == "workflow_dispatch":
        require(os.environ.get("PLAN_RESULT") == "skipped", "dispatch plan must be skipped")
        return "run"
    require(event_name == "pull_request" and os.environ.get("PLAN_RESULT") == "success",
            "plan not successful")
    directory = Path(os.environ["RUNNER_TEMP"]) / "source-qualification-plan"
    require(not directory.is_symlink() and directory.is_dir(), "plan artifact missing")
    require({entry.name for entry in directory.iterdir()} == {"plan.json", "matrix.json"},
            "plan artifact inventory mismatch")
    plan_path = directory / "plan.json"
    data = regular(plan_path, MAX_PLAN)
    require(hashlib.sha256(data).hexdigest() == digest(os.environ.get("PLAN_SHA256", "")),
            "plan artifact digest mismatch")
    plan = parse_json(data)
    matrix_data = regular(directory / "matrix.json", MAX_PLAN)
    require(hashlib.sha256(matrix_data).hexdigest() == digest(os.environ.get("MATRIX_SHA256", ""))
            and parse_json(matrix_data) == plan["matrix"], "matrix artifact mismatch")
    event = parse_json(regular(Path(os.environ["GITHUB_EVENT_PATH"]), MAX_PLAN))
    pull = event["pull_request"]
    identities = (pull["head"]["repo"]["id"], pull["base"]["repo"]["id"], event["repository"]["id"])
    require(all(type(value) is int and 0 < value <= 18446744073709551615 for value in identities)
            and identities[1] == identities[2], "invalid repository identity")
    run_id, attempt = os.environ["GITHUB_RUN_ID"], os.environ["GITHUB_RUN_ATTEMPT"]
    require(re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[1-9][0-9]*", attempt),
            "invalid run identity")
    run_key = f"r{run_id}-a{attempt}"
    require(plan["schema"] == 2 and plan["run_key"] == run_key
            and plan["plan_id"] == "plan-" + run_key
            and plan["event"] in ("pull_request", "fork") and plan["trust"] == "pr"
            and plan["base"] == pull["base"]["sha"] and plan["head"] == pull["head"]["sha"],
            "plan event identity mismatch")
    generator = plan["generator"]
    require(generator["version"] == os.environ["CONTROLLER_VERSION"]
            and generator["target"] == os.environ["CONTROLLER_TARGET"]
            and generator["sha256"] == digest(os.environ.get("BINARY_SHA256", "")),
            "plan controller identity mismatch")
    routes = json.loads(os.environ["CLI_ROUTES"])
    require(isinstance(routes, list) and routes and len(routes) == len(set(routes))
            and all(isinstance(route, str) and route in (CLI, CLI + "__hosted", CLI + "__local")
                    for route in routes), "invalid CLI route")
    jobs = plan["jobs"]
    require(isinstance(jobs, list), "invalid plan jobs")
    selected = [job for job in jobs if isinstance(job, dict) and job.get("job_id") in routes]
    require(len(selected) == len(routes) and {job["job_id"] for job in selected} == set(routes),
            "CLI route missing or duplicated")
    for job in selected:
        if job["job_id"] != CLI:
            require(job.get("lane", {}).get("logical_job_id") == CLI, "CLI logical route mismatch")
    dispositions = {job.get("disposition") for job in selected}
    require(dispositions in ({"run"}, {"not_affected"}), "invalid CLI disposition")
    return "run" if dispositions == {"run"} else "skip"


def gather():
    event = os.environ.get("GITHUB_EVENT_NAME", "")
    require(os.environ.get("SELECTION_RESULT") == "success", "selection failed")
    decision = os.environ.get("DECISION", "")
    results = [os.environ.get(name, "") for name in
               ("ORDINARY_RESULT", "KEEP_RESULT", "GENERATION_RESULT")]
    if event == "pull_request":
        require(os.environ.get("PLAN_RESULT") == "success", "plan failed")
    else:
        require(event == "workflow_dispatch" and os.environ.get("PLAN_RESULT") == "skipped",
                "invalid dispatch")
    if decision == "skip":
        require(event == "pull_request" and all(result == "skipped" for result in results),
                "unexpected producer result")
    else:
        require(decision == "run" and all(result == "success" for result in results),
                "producer failed or skipped")


def main():
    require(len(sys.argv) == 2 and sys.argv[1] in ("select", "gather"), "invalid mode")
    if sys.argv[1] == "gather":
        gather()
    else:
        decision = select()
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write("decision=" + decision + "\n")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError, json.JSONDecodeError) as error:
        print("source qualification routing rejected: " + str(error), file=sys.stderr)
        sys.exit(1)
