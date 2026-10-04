"""Fixed read-only forge observations, including annotated tag peeling."""
import hashlib
import io
import subprocess
import zipfile
from urllib.parse import quote


def forge_api(endpoint):
    require(endpoint.startswith("repos/"), "forge_endpoint")
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("CARGO_", "ACTIONS_ID_TOKEN_")) and
                   (not key.endswith("_TOKEN") or key == "GH_TOKEN")}
    environment["GH_HOST"] = "github.com"
    result = subprocess.run(["gh", "api", "--hostname", "github.com", "--include", endpoint],
                            capture_output=True, env=environment, timeout=40, check=False)
    require(len(result.stdout) <= 4 * 1024 * 1024, "forge_response_size")
    header, separator, body = result.stdout.partition(b"\r\n\r\n")
    if not separator:
        header, separator, body = result.stdout.partition(b"\n\n")
    require(separator, "forge_response_headers")
    status = header.splitlines()[0].split()
    require(len(status) >= 2 and status[1].isdigit(), "forge_response_status")
    code = int(status[1])
    if code == 404:
        require(result.returncode != 0, "forge_404_status")
        return None
    require(code == 200 and result.returncode == 0, "forge_unavailable")
    return decode_json(body)


def forge_package(approved, name, require_release):
    repository = approved["repository"]
    tag = approved["tags"][name]
    ref = forge_api(f"repos/{repository}/git/ref/tags/{quote(tag, safe='')}")
    if ref is None:
        orphan = forge_api(f"repos/{repository}/releases/tags/{quote(tag, safe='')}")
        require(orphan is None, "release_without_tag_collision")
        require(not require_release, "tag_missing")
        return {"status": "absent", "tag": tag, "target": None, "release_url": None}
    require(ref.get("ref") == "refs/tags/" + tag, "forge_tag_identity")
    obj = ref.get("object", {})
    seen = set()
    for _ in range(8):
        sha = obj.get("sha")
        require(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha) and
                sha not in seen, "forge_tag_object")
        seen.add(sha)
        if obj.get("type") == "commit":
            break
        require(obj.get("type") == "tag", "forge_tag_type")
        annotated = forge_api(f"repos/{repository}/git/tags/{sha}")
        require(isinstance(annotated, dict) and annotated.get("sha") == sha, "forge_tag_peel")
        obj = annotated.get("object", {})
    else:
        raise ReconcileError("forge_tag_depth")
    require(sha == approved["source_sha"], "tag_source_collision")
    release = forge_api(f"repos/{repository}/releases/tags/{quote(tag, safe='')}")
    if release is None:
        require(not require_release, "release_missing")
        return {"status": "tag_only", "tag": tag, "target": sha, "release_url": None}
    require(release.get("tag_name") == tag and release.get("draft") is False, "release_identity")
    url = f"https://github.com/{repository}/releases/tag/{quote(tag, safe='/')}"
    require(release.get("html_url") == url, "release_url")
    return {"status": "verified", "tag": tag, "target": sha, "release_url": url}


def _producer_job(approved, run_id, attempt, producer_job):
    require(isinstance(producer_job, str) and
            re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,99}", producer_job),
            "artifact_producer_job")
    repository = approved["repository"]
    selected = []
    for page in range(1, 11):
        response = forge_api(
            f"repos/{repository}/actions/runs/{run_id}/attempts/{attempt}/jobs"
            f"?per_page=100&page={page}"
        )
        require(isinstance(response, dict) and isinstance(response.get("jobs"), list),
                "artifact_jobs")
        jobs = response["jobs"]
        selected.extend(job for job in jobs if job.get("name") == producer_job)
        if len(jobs) < 100:
            break
    else:
        raise ReconcileError("artifact_job_pagination_limit")
    require(len(selected) == 1, "artifact_producer_missing_or_duplicate")
    job = selected[0]
    conclusions = ("success", "failure", "cancelled") if producer_job in {
        "release-registry-publish", "release-forge-publish"} else ("success",)
    require(type(job.get("id")) is int and job["id"] > 0 and
            job.get("name") == producer_job and type(job.get("run_id")) is int and
            type(job.get("run_attempt")) is int and job.get("run_id") == int(run_id) and
            job.get("run_attempt") == int(attempt) and
            job.get("head_sha") == os.environ["GITHUB_SHA"] and
            job.get("status") == "completed" and job.get("conclusion") in conclusions,
            "artifact_producer_identity")
    return {"id": job["id"], "name": producer_job, "conclusion": job["conclusion"]}


def _artifact_upload_binding(producer_job):
    prefixes = {
        "release-package": "RELEASE_PACKAGE_ARTIFACT",
        "release-preflight": "RELEASE_PREFLIGHT_ARTIFACT",
        "release-registry-publish": "RELEASE_REGISTRY_RECEIPT_ARTIFACT",
        "release-forge-publish": "RELEASE_FORGE_RECEIPT_ARTIFACT",
        "release-preparation-source": "RELEASE_PREPARE_ARTIFACT",
    }
    require(producer_job in prefixes, "artifact_binding_producer")
    prefix = prefixes[producer_job]
    identifier = os.environ.get(prefix + "_ID", "")
    digest = os.environ.get(prefix + "_DIGEST", "")
    require(re.fullmatch(r"[1-9][0-9]*", identifier) and
            re.fullmatch(r"[0-9a-f]{64}", digest), "artifact_upload_binding")
    return int(identifier), "sha256:" + digest


def _artifact_raw(approved, run_id, attempt, name, run, producer_job, binding):
    repository = approved["repository"]
    selected = []
    for page in range(1, 11):
        response = forge_api(f"repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100&page={page}")
        require(isinstance(response, dict) and isinstance(response.get("artifacts"), list), "artifact_list")
        artifacts = response["artifacts"]
        selected.extend(item for item in artifacts if item.get("name") == name)
        if len(artifacts) < 100:
            break
    else:
        raise ReconcileError("artifact_pagination_limit")
    require(len(selected) == 1, "artifact_missing_or_duplicate")
    artifact = selected[0]
    identifier = artifact.get("id")
    size = artifact.get("size_in_bytes")
    digest = artifact.get("digest")
    require(type(identifier) is int and identifier > 0 and type(size) is int and
            0 < size <= 256 * 1024 * 1024 and artifact.get("expired") is False and
            isinstance(digest, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", digest), "artifact_identity")
    require((identifier, digest) == binding, "artifact_upload_binding_mismatch")
    origin = artifact.get("workflow_run", {})
    require(type(origin.get("id")) is int and
            type(origin.get("repository_id")) is int and
            type(origin.get("head_repository_id")) is int and origin.get("id") == int(run_id) and origin.get("head_sha") == os.environ["GITHUB_SHA"] and
            origin.get("repository_id") == run["repository"]["id"] and
            origin.get("head_repository_id") == run.get("head_repository", {}).get("id"),
            "artifact_origin")
    producer = _producer_job(approved, run_id, attempt, producer_job)
    result = subprocess.run(["gh", "api", "--hostname", "github.com",
                             f"repos/{repository}/actions/artifacts/{identifier}/zip"],
                            capture_output=True, timeout=60, check=False)
    require(result.returncode == 0 and len(result.stdout) <= 256 * 1024 * 1024, "artifact_download")
    require("sha256:" + hashlib.sha256(result.stdout).hexdigest() == digest, "artifact_digest_mismatch")
    receipt = {"id": identifier, "digest": digest, "name": name, "producer_job": producer}
    conclusions = ("success", "failure", "cancelled") if producer_job in {
        "release-registry-publish", "release-forge-publish"} else ("success",)
    validate_artifact_identity(receipt, identifier, digest, name, producer_job, conclusions)
    return result.stdout, receipt


def _evidence_member(blob):
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        members = archive.infolist()
        require(len(members) == 1 and members[0].filename == "evidence.json" and
                members[0].file_size <= 16 * 1024 * 1024 and not members[0].is_dir() and
                (members[0].external_attr >> 16) & 0o170000 in (0, 0o100000), "artifact_members")
        return archive.read(members[0])


def artifact_bytes(approved, artifact_name, producer_job):
    """Authenticate one fixed producer artifact before parsing the downloaded buffer."""
    binding = _artifact_upload_binding(producer_job)
    run_id = os.environ["GITHUB_RUN_ID"]
    attempt = os.environ["GITHUB_RUN_ATTEMPT"]
    require(re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[1-9][0-9]*", attempt), "artifact_run")
    repository = approved["repository"]
    run = forge_api(f"repos/{repository}/actions/runs/{run_id}/attempts/{attempt}")
    require(isinstance(run, dict) and type(run.get("id")) is int and
            type(run.get("run_attempt")) is int and run.get("id") == int(run_id) and
            run.get("run_attempt") == int(attempt) and run.get("head_sha") == os.environ["GITHUB_SHA"] and
            run.get("repository", {}).get("full_name") == repository and
            run.get("head_repository", {}).get("full_name") == repository and
            type(run.get("head_repository", {}).get("id")) is int and
            run["head_repository"]["id"] > 0 and
            type(run.get("repository", {}).get("id")) is int and
            run["repository"]["id"] > 0, "artifact_run_authority")
    workflow_id = run.get("workflow_id")
    require(type(workflow_id) is int and workflow_id > 0, "artifact_workflow_id")
    workflow = forge_api(f"repos/{repository}/actions/workflows/{workflow_id}")
    require(isinstance(workflow, dict) and workflow.get("path") == ".github/workflows/release.yml", "artifact_workflow")
    name = artifact_name
    require(isinstance(name, str) and
            re.fullmatch(r"velnor-[A-Za-z0-9][A-Za-z0-9_.-]*-r[1-9][0-9]*-a[1-9][0-9]*", name),
            "artifact_name")
    return _artifact_raw(approved, run_id, attempt, name, run, producer_job,
                         binding)


def artifact_evidence(approved, artifact_name=None, producer_job=None):
    require((artifact_name is None) == (producer_job is None), "artifact_binding")
    if artifact_name is None:
        artifact_name = f"velnor-release-preflight-r{os.environ['GITHUB_RUN_ID']}-a{os.environ['GITHUB_RUN_ATTEMPT']}"
        producer_job = "release-preflight"
    blob, receipt = artifact_bytes(approved, artifact_name, producer_job)
    return _evidence_member(blob), receipt
