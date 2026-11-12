# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import http.client
import json
import os
from pathlib import Path
import re
import signal
import ssl
import sys
import tempfile
import time
import tomllib

REPOSITORY = "tailrocks/velnor-new"
REPOSITORY_ID = 1390620900
REPOSITORY_OWNER_ID = 78806509
WORKFLOW = ".github/workflows/product-release-generator.yml"
LIMIT = 2 * 1024 * 1024


class GitHub:
    def __init__(self):
        self.deadline = time.monotonic() + 120
        self.token = os.environ["GH_TOKEN"]

    def get(self, path):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0 or not (path.startswith("/repos/" + REPOSITORY + "/")
                                  or path == "/repos/" + REPOSITORY
                                  or path == "/users/donbeave"):
            raise ValueError("source preview API boundary")
        connection = http.client.HTTPSConnection(
            "api.github.com", timeout=min(30, remaining),
            context=ssl.create_default_context(),
        )
        try:
            connection.request("GET", path, headers={
                "Accept": "application/vnd.github+json",
                "Authorization": "Bearer " + self.token,
                "User-Agent": "velnor-source-preview",
            })
            response = connection.getresponse()
            if response.status != 200:
                raise ValueError("source preview API response")
            body = bytearray()
            while True:
                remaining = self.deadline - time.monotonic()
                if remaining <= 0:
                    raise ValueError("source preview API deadline")
                if connection.sock is not None:
                    connection.sock.settimeout(min(30, remaining))
                chunk = response.read(min(65536, LIMIT + 1 - len(body)))
                if not chunk:
                    break
                body.extend(chunk)
                if len(body) > LIMIT:
                    raise ValueError("source preview API size")
            return json.loads(body, object_pairs_hook=unique_object)
        finally:
            connection.close()


def unique_object(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError("source preview duplicate JSON field")
        result[name] = value
    return result


def positive(value):
    if type(value) is not int or not 0 < value <= 9007199254740991:
        raise ValueError("source preview integer")
    return value


def verify_jobs(client, run, expected):
    result = client.get(
        "/repos/" + REPOSITORY + "/actions/runs/" + str(positive(run.get("id")))
        + "/attempts/" + str(positive(run.get("run_attempt")))
        + "/jobs?per_page=100"
    )
    jobs = result.get("jobs")
    if (not isinstance(jobs, list) or not jobs or len(jobs) >= 100
            or result.get("total_count") != len(jobs)):
        raise ValueError("source preview job inventory")
    ids = [positive(job.get("id")) for job in jobs]
    if len(set(ids)) != len(ids):
        raise ValueError("source preview duplicate job")
    selected = {}
    for name in expected:
        matches = [job for job in jobs if job.get("name") == name]
        if len(matches) != 1:
            raise ValueError("source preview required job missing")
        job = matches[0]
        if (job.get("status") != "completed" or job.get("conclusion") != "success"
                or job.get("run_id") != run["id"]
                or job.get("run_attempt") != run["run_attempt"]
                or job.get("head_sha") != run["head_sha"]):
            raise ValueError("source preview required job failed")
        selected[name] = job
    return selected


def successful_run(client, source, path, event, expected):
    result = client.get(
        "/repos/" + REPOSITORY + "/actions/workflows/" + path.rsplit("/", 1)[1]
        + "/runs?head_sha=" + source + "&event=" + event
        + "&status=success&per_page=20"
    )
    runs = result.get("workflow_runs")
    if not isinstance(runs, list) or not runs or len(runs) > 20:
        raise ValueError("source preview qualification missing")
    accepted = []
    for run in runs:
        reference = run.get("path", "").split("@", 1)
        if (run.get("head_sha") == source and run.get("head_branch") == "main"
                and run.get("event") == event and run.get("status") == "completed"
                and run.get("conclusion") == "success"
                and reference[0] == path
                and (len(reference) == 1 or reference[1] in ("main", "refs/heads/main"))
                and run.get("head_repository", {}).get("full_name") == REPOSITORY
                and run.get("head_repository", {}).get("id") == REPOSITORY_ID):
            accepted.append(run)
    if not accepted:
        raise ValueError("source preview qualification identity")
    run = max(accepted, key=lambda value: positive(value.get("id")))
    verify_jobs(client, run, expected)
    return {
        "workflow_path": path,
        "workflow_ref": REPOSITORY + "/" + path + "@refs/heads/main",
        "workflow_sha": source,
        "run_id": positive(run.get("id")),
        "run_attempt": positive(run.get("run_attempt")),
    }


def producer_artifact(client, source, run_id, attempt, target):
    run_id = positive(run_id)
    attempt = positive(attempt)
    path = "/repos/" + REPOSITORY + "/actions/runs/" + str(run_id)
    run = client.get(path + "/attempts/" + str(attempt))
    if (run.get("id") != run_id or run.get("run_attempt") != attempt
            or run.get("head_sha") != source or run.get("head_branch") != "main"
            or run.get("event") != "push" or run.get("status") != "completed"
            or run.get("conclusion") != "success"
            or run.get("path", "").split("@", 1)[0] != WORKFLOW
            or run.get("head_repository", {}).get("full_name") != REPOSITORY
            or run.get("head_repository", {}).get("id") != REPOSITORY_ID):
        raise ValueError("SDK preview main producer identity")
    label = {"linux": "Linux", "darwin": "Darwin"}[target]
    jobs = verify_jobs(client, run, ("Build source preview " + label, "Qualify source preview " + label))
    inventory = client.get(path + "/artifacts?per_page=100")
    artifacts = inventory.get("artifacts")
    if (not isinstance(artifacts, list) or len(artifacts) >= 100
            or inventory.get("total_count") != len(artifacts)):
        raise ValueError("SDK preview producer artifact inventory")
    matches = [value for value in artifacts
               if value.get("name") == "source-preview-payload-" + target + "-" + str(attempt)]
    if len(matches) != 1:
        raise ValueError("SDK preview producer artifact ambiguous")
    artifact = matches[0]
    if (type(artifact.get("id")) is not int or artifact["id"] <= 0
            or artifact.get("expired") is not False
            or not re.fullmatch("sha256:[0-9a-f]{64}", artifact.get("digest", ""))
            or artifact.get("workflow_run", {}).get("id") != run_id
            or artifact.get("workflow_run", {}).get("head_sha") != source):
        raise ValueError("SDK preview producer artifact identity")
    return artifact, jobs["Qualify source preview " + label]["id"]


def source_version(client, source):
    manifest_blob = client.get("/repos/" + REPOSITORY + "/contents/Cargo.toml?ref=" + source)
    if manifest_blob.get("path") != "Cargo.toml" or manifest_blob.get("encoding") != "base64":
        raise ValueError("source preview version source")
    version = tomllib.loads(base64.b64decode(manifest_blob["content"], validate=False).decode())
    version = version["workspace"]["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("SDK preview release version")
    return version


def qualification_proofs(client, source):
    return {
        "ci": successful_run(client, source, ".github/workflows/ci.yml", "push", ("Required verdict",)),
        "qualification": successful_run(
            client, source, ".github/workflows/source-qualification.yml", "workflow_dispatch",
            ("Source qualification ordinary harness", "Source qualification keep harness",
             "Source qualification generation and repository checks", "Source qualification"),
        ),
    }


def verify_repository(client):
    repository = client.get("/repos/" + REPOSITORY)
    if (repository.get("id") != REPOSITORY_ID or repository.get("full_name") != REPOSITORY
            or repository.get("default_branch") != "main" or repository.get("owner", {}).get("id") != REPOSITORY_OWNER_ID):
        raise ValueError("SDK preview repository identity")


def producer_inputs(client, source):
    verify_repository(client)
    qualification_proofs(client, source)
    jobs = tuple(f"{action} source preview {target}" for action in ("Build", "Qualify") for target in ("Linux", "Darwin"))
    run = successful_run(client, source, WORKFLOW, "push", jobs)
    targets = {}
    for target in ("linux", "darwin"):
        artifact, job_id = producer_artifact(client, source, run["run_id"], run["run_attempt"], target)
        targets[target] = {"producer_run_id": run["run_id"], "producer_run_attempt": run["run_attempt"],
                           "payload_artifact_id": artifact["id"], "payload_archive_sha256": artifact["digest"][7:],
                           "qualification_job_id": job_id}
    return targets


def admit(client):
    preview_authority()
    verify_repository(client)
    source = os.environ["GITHUB_SHA"]
    version = source_version(client, source)
    proof, tag_sha, expected_ref = admit_tag(client, source, version)
    if version != proof["version"]:
        raise ValueError("SDK preview tag version mismatch")
    metadata = {
        "schema": 3, "repository": REPOSITORY, "version": version,
        "source_sha": source, "tag": "sdk-preview-" + source,
        "source": {
            "workflow_path": WORKFLOW, "workflow_ref": expected_ref,
            "workflow_sha": source,
            "run_id": positive(int(os.environ["GITHUB_RUN_ID"])),
            "run_attempt": positive(int(os.environ["GITHUB_RUN_ATTEMPT"])),
        },
        "targets": {target: {
            "producer": {key: item[key] for key in ("producer_run_id", "producer_run_attempt", "payload_artifact_id", "payload_archive_sha256")},
            "qualification_job_id": item["qualification_job_id"],
        } for target, item in proof["targets"].items()},
        **qualification_proofs(client, source),
    }
    artifacts = {}
    for target in ("linux", "darwin"):
        item = proof["targets"][target]
        artifact, job_id = producer_artifact(client, source, item["producer_run_id"], item["producer_run_attempt"], target)
        if (artifact["id"] != item["payload_artifact_id"] or artifact["digest"] != "sha256:" + item["payload_archive_sha256"]
                or job_id != item["qualification_job_id"]):
            raise ValueError("SDK preview annotated producer changed")
        artifacts[target] = artifact
    root = Path(os.environ["RUNNER_TEMP"])
    with tempfile.TemporaryDirectory(prefix="sdk-preview-input-", dir=root) as temporary:
        for target, triple in (("linux", "x86_64-unknown-linux-gnu"), ("darwin", "aarch64-apple-darwin")):
            staged = Path(temporary) / target
            download_verified_payload(client, artifacts[target], staged, triple)
        if tag_reference(client, source) != tag_sha:
            raise ValueError("SDK preview tag moved")
        verify_default_ancestry(client, source)
        for target, triple in (("linux", "x86_64-unknown-linux-gnu"), ("darwin", "aarch64-apple-darwin")):
            os.link(Path(temporary) / target, root / ("velnor-actions-" + triple), follow_symlinks=False)
    return metadata


def main():
    signal.signal(signal.SIGALRM, deadline_alarm)
    signal.setitimer(signal.ITIMER_REAL, 120)
    metadata = admit(GitHub())
    path = Path(os.environ["RUNNER_TEMP"]) / "source-preview-metadata.json"
    with path.open("x", encoding="utf-8") as output:
        json.dump(metadata, output, sort_keys=True, separators=(",", ":"))
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write("source_sha=" + metadata["source_sha"] + "\n")


def deadline_alarm(_number, _frame):
    raise TimeoutError("source preview deadline")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, http.client.HTTPException, zipfile.BadZipFile):
        print("source preview admission failed", file=sys.stderr)
        sys.exit(1)
