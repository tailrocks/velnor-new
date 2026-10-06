"""Fixed read-only admission; API artifacts are data, never executable input."""
import hashlib
import io
import json
import os
import re
import subprocess
import zipfile
from urllib.parse import quote, urlencode


def require(condition, reason):
    if not condition:
        raise RuntimeError("release_admission:" + reason)


def strict_json(blob):
    def object_pairs(pairs):
        value = {}
        for key, item in pairs:
            require(key not in value, "duplicate_json_key")
            value[key] = item
        return value

    def reject_constant(value):
        raise RuntimeError("release_admission:nonfinite_json:" + value)

    return json.loads(blob, object_pairs_hook=object_pairs, parse_constant=reject_constant)


def natural(value, positive=False):
    return type(value) is int and value >= (1 if positive else 0)


def planning_gh():
    executable = os.environ.get("VELNOR_ADMISSION_PLANNING_GH", "")
    require(executable and os.path.isabs(executable) and
            os.path.normpath(executable) == executable and
            os.path.basename(executable) == "gh", "planning_gh_path")
    return executable


def api(path, binary=False):
    executable = planning_gh()
    result = subprocess.run(
        [executable, "api", "--hostname", "github.com", "--method", "GET", path], check=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120,
    )
    require(len(result.stdout) <= 16 * 1024 * 1024, "oversize_api")
    return result.stdout if binary else strict_json(result.stdout)


def pages(path, field):
    values = []
    for page in range(1, 101):
        separator = "&" if "?" in path else "?"
        result = api(path + separator + urlencode({"per_page": 100, "page": page}))
        batch = result[field]
        require(isinstance(batch, list), "bad_page")
        values.extend(batch)
        if len(batch) < 100:
            return values
    raise RuntimeError("release_admission:pagination_limit")


def document(artifact, filename, run, repo, sha):
    require(artifact["expired"] is False and natural(artifact["id"], True), "expired_artifact")
    binding = artifact["workflow_run"]
    require(binding["id"] == run["id"] and binding["head_sha"] == sha,
            "artifact_run_binding")
    require(binding["repository_id"] == run["repository"]["id"], "artifact_repo_binding")
    blob = api(f"repos/{repo}/actions/artifacts/{artifact['id']}/zip", binary=True)
    digest = artifact["digest"]
    require(isinstance(digest, str) and digest == "sha256:" + hashlib.sha256(blob).hexdigest(),
            "artifact_digest")
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        members = [item for item in archive.infolist() if item.filename == filename]
        require(len(members) == 1, "artifact_member")
        require(members[0].file_size <= 8 * 1024 * 1024, "oversize_report")
        return strict_json(archive.read(members[0]))


def evidence(run, repo, sha):
    key = f"r{run['id']}-a{run['run_attempt']}"
    artifacts = pages(f"repos/{repo}/actions/runs/{run['id']}/artifacts", "artifacts")
    documents = {}
    for kind, filename in (("final", "final-report.json"), ("plan", "plan.json")):
        matches = [entry for entry in artifacts if entry["name"] == f"velnor-{kind}-{key}"]
        require(len(matches) == 1, "missing_or_duplicate_" + kind)
        documents[kind] = document(matches[0], filename, run, repo, sha)
    final, plan = documents["final"], documents["plan"]
    for value in (final, plan):
        require(type(value["schema"]) is int and value["schema"] == 1 and value["run_key"] == key
                and value["plan_id"] == "plan-" + key, "document_identity")
    require(final["report_id"] == "final-" + key, "final_identity")
    require(plan["head"] == sha and plan["event"] == "push"
            and plan["trust"] == "trusted", "plan_source")
    verify_obligations(plan, final)


def verify_baseline(proof):
    require(isinstance(proof, dict) and set(proof) == {
        "source_commit", "run_id", "artifact_id", "artifact_name", "manifest_digest"},
        "baseline_shape")
    require(re.fullmatch(r"[0-9a-f]{40}", proof["source_commit"]) is not None
            and natural(proof["run_id"], True) and natural(proof["artifact_id"], True),
            "baseline_identity")
    require(re.fullmatch(r"velnor-baseline-r[0-9]+-a[0-9]+", proof["artifact_name"]) is not None
            and re.fullmatch(r"b3-[0-9a-f]{64}", proof["manifest_digest"]) is not None,
            "baseline_digest")


def verify_obligations(plan, final):
    obligations = plan["obligations"]
    require(isinstance(obligations, list) and bool(obligations), "empty_obligations")
    ids = [item["task_id"] for item in obligations]
    require(len(set(ids)) == len(ids) and sorted(ids) == plan["task_ids"], "obligation_universe")
    require(all(item["decision"] in ("execute", "covered_by_trusted_baseline")
                for item in obligations), "unproved_obligation")
    for item in obligations:
        for field in ("task_digest", "input_digest", "closure_digest"):
            require(re.fullmatch(r"b3-[0-9a-f]{64}", item[field]) is not None, "obligation_digest")
        if item["decision"] == "covered_by_trusted_baseline":
            verify_baseline(item.get("baseline_proof"))
    counts = final["counts"]
    require(set(counts) == {"selected", "reused", "executed", "empty_partition", "covered",
                            "failed", "cancelled", "blocked", "not_run"}
            and all(natural(value) for value in counts.values()), "invalid_counts")
    require(final["status"] == "passed" and counts["selected"] == len(obligations),
            "no_substantive_success")
    require(all(counts[name] == 0 for name in ("failed", "cancelled", "blocked", "not_run")),
            "failed_obligations")
    covered = sum(item["decision"] == "covered_by_trusted_baseline" for item in obligations)
    require(counts["covered"] == covered
            and counts["executed"] + counts["reused"] + covered == len(obligations),
            "incomplete_obligations")
    jobs = final["required_job_results"]
    require(bool(jobs) and len({item["job_id"] for item in jobs}) == len(jobs), "job_inventory")
    for job in jobs:
        owned = [item for item in obligations if item["job_id"] == job["job_id"]]
        allowed_skip = bool(owned) and all(item["decision"] == "covered_by_trusted_baseline"
                                           for item in owned) and job["job_id"].startswith(("rust-", "tofu-", "workload-"))
        require(job["conclusion"] == "success"
                or (job["conclusion"] == "skipped" and allowed_skip), "unsuccessful_validator")
    require({item["job_id"] for item in obligations} <= {item["job_id"] for item in jobs},
            "missing_obligation_job")


def qualify(run, repo, sha, branch, workflow_id):
    require(natural(run["id"], True) and natural(run["run_attempt"], True)
            and natural(workflow_id, True), "run_identity")
    require(run["repository"]["full_name"] == repo
            and run["head_repository"]["full_name"] == repo, "foreign_run")
    require(run["head_sha"] == sha and run["head_branch"] == branch
            and run["event"] == "push" and run["workflow_id"] == workflow_id
            and run["path"] == ".github/workflows/ci.yml", "run_source")
    require(run["status"] == "completed" and run["conclusion"] == "success", "unfinished_ci")
    jobs = pages(f"repos/{repo}/actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs", "jobs")
    required = [job for job in jobs if job["name"] == "Required"]
    require(len(required) == 1, "missing_or_duplicate_required")
    job = required[0]
    require(job["run_id"] == run["id"] and job["run_attempt"] == run["run_attempt"]
            and job["head_sha"] == sha
            and job["status"] == "completed" and job["conclusion"] == "success", "required_failed")
    evidence(run, repo, sha)


def caller_event(branch):
    policy = os.environ["ADMISSION_EVENT_POLICY"]
    event, ref = os.environ["GITHUB_EVENT_NAME"], os.environ["GITHUB_REF"]
    on_branch = ref == "refs/heads/" + branch
    on_tag = ref.startswith("refs/tags/")
    policies = {
        "rust": on_branch and event in ("workflow_dispatch", "push"),
        "default-branch": on_branch and event in ("workflow_dispatch", "push", "schedule"),
        "oci-tag": on_tag and event in ("workflow_dispatch", "push"),
        "desktop-tag": on_tag and event == "push",
    }
    require(policy in policies and policies[policy], "caller_event")


def main():
    repo = os.environ["APPROVED_REPOSITORY"]
    sha = os.environ["APPROVED_SOURCE_SHA"]
    branch = os.environ["APPROVED_DEFAULT_BRANCH"]
    require(re.fullmatch(r"[0-9a-f]{40}", sha) is not None, "source_sha")
    require(os.environ["GITHUB_REPOSITORY"] == repo, "foreign_caller")
    caller_event(branch)
    require(api(f"repos/{repo}")["default_branch"] == branch, "default_branch_drift")
    comparison = api(f"repos/{repo}/compare/{sha}...{quote(branch, safe='')}")
    require(comparison["merge_base_commit"]["sha"] == sha
            and comparison["status"] in ("ahead", "identical"), "source_not_default_ancestor")
    workflow = api(f"repos/{repo}/actions/workflows/ci.yml")
    require(workflow["path"] == ".github/workflows/ci.yml" and workflow["state"] == "active", "ci_identity")
    runs = pages(f"repos/{repo}/actions/workflows/{workflow['id']}/runs?"
                 + urlencode({"event": "push", "branch": branch, "head_sha": sha}), "workflow_runs")
    require(bool(runs), "no_exact_ci")
    newest = max(runs, key=lambda run: run["id"])
    run = api(f"repos/{repo}/actions/runs/{newest['id']}")
    qualify(run, repo, sha, branch, workflow["id"])
    print("Admitted protected CI", run["id"], run["run_attempt"], sha)


if __name__ == "__main__":
    main()
