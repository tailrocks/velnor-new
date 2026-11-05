# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import datetime
import json
import os
import re
import time
import urllib.error
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("unexpected redirect")


def main():
    mono = time.clock_gettime_ns(time.CLOCK_BOOTTIME)
    now = time.time_ns()
    repo = os.environ["GITHUB_REPOSITORY"]
    run = os.environ["GITHUB_RUN_ID"]
    attempt = os.environ["GITHUB_RUN_ATTEMPT"]
    job = os.environ["GITHUB_JOB"]
    sha = os.environ["GITHUB_SHA"]
    names = {"release-publish": "Release publish", "release-publish-bootstrap": "Release bootstrap publish"}
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo) or not re.fullmatch(r"[1-9][0-9]*", run) or attempt != "1" or job not in names or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("invalid workflow identity")
    base = "https://api.github.com/repos/" + repo + "/actions/runs/" + run
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    headers = {"Authorization": "Bearer " + os.environ["GH_TOKEN"], "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}

    def get(url):
        with opener.open(urllib.request.Request(url, headers=headers), timeout=20) as response:
            data = response.read(1048577)
        if len(data) > 1048576:
            raise ValueError("oversized GitHub response")
        return json.loads(data, object_pairs_hook=unique)

    run_info = get(base + "/attempts/" + attempt)
    if run_info.get("id") != int(run) or run_info.get("run_attempt") != int(attempt) or run_info.get("head_sha") != sha or run_info.get("repository", {}).get("full_name", "").lower() != repo.lower():
        raise ValueError("run identity mismatch")
    found = []
    total = None
    for page in range(1, 101):
        result = get(base + "/attempts/" + attempt + "/jobs?per_page=100&page=" + str(page))
        jobs = result.get("jobs")
        count = result.get("total_count")
        if not isinstance(jobs, list) or type(count) is not int or count < 0 or count > 10000 or (total is not None and total != count):
            raise ValueError("invalid job inventory")
        total = count
        found.extend(jobs)
        if len(found) >= count:
            break
        if len(jobs) != 100:
            raise ValueError("incomplete job inventory")
    if total is None or len(found) != total or any(type(entry.get("id")) is not int or entry["id"] <= 0 for entry in found) or len({entry["id"] for entry in found}) != total:
        raise ValueError("ambiguous job inventory")
    matching = [entry for entry in found if entry.get("name") == names[job]]
    if len(matching) != 1 or matching[0].get("run_id") != int(run) or matching[0].get("run_attempt") != int(attempt) or matching[0].get("head_sha") != sha or matching[0].get("status") != "in_progress":
        raise ValueError("job identity mismatch")
    started = datetime.datetime.fromisoformat(matching[0]["started_at"].replace("Z", "+00:00"))
    if started.tzinfo is None:
        raise ValueError("missing job timezone")
    start_ns = int(started.timestamp()) * 1000000000
    if start_ns > now:
        raise ValueError("job start is in the future")
    deadline = mono + min(1800000000000, start_ns + 1800000000000 - now)
    if deadline <= time.clock_gettime_ns(time.CLOCK_BOOTTIME):
        raise ValueError("job budget exhausted")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"budget=v1:{run}:{attempt}:{job}:{deadline}\n")


def unique(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON key")
        value[key] = item
    return value


if __name__ == "__main__":
    main()
