"""Stdlib-only OCI delivery gates embedded into generated workflow steps."""

import json
import os
import re
import subprocess
import sys
import urllib.parse
import urllib.error
import urllib.request
from pathlib import Path


class GateError(Exception):
    """A closed OCI gate diagnostic."""


def die(code):
    print("oci_" + code, file=sys.stderr)
    raise SystemExit(1)


def need(condition, code):
    if not condition:
        raise GateError(code)


def required(name):
    value = os.environ.get(name, "")
    need(value != "", "missing_" + name.lower())
    return value


def duplicate_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise GateError("duplicate_json_key")
        value[key] = item
    return value


def json_document(raw, code="malformed_json"):
    try:
        return json.loads(raw, object_pairs_hook=duplicate_object)
    except (TypeError, ValueError, GateError) as error:
        if isinstance(error, GateError):
            raise
        raise GateError(code) from error


def json_file(path, code="malformed_json"):
    try:
        return json_document(path.read_text(encoding="utf-8"), code)
    except OSError as error:
        raise GateError("read_json") from error


def lower_sha(value):
    return isinstance(value, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", value) is not None


def source_sha(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value) is not None


def semver(value, tag=False):
    prefix = r"v" if tag else r""
    pattern = prefix + r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    identifier = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    pattern += r"(?:-" + identifier + r"(?:\." + identifier + r")*)?"
    pattern += r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    return re.fullmatch(pattern, value or "") is not None


def oci_version(value):
    return semver(value) and "+" not in value and len(value) <= 128


def safe_arch(value):
    return value in {"amd64", "arm64"}


def safe_id(value):
    return re.fullmatch(r"[a-z][a-z0-9-]{0,127}", value or "") is not None


def run(argv):
    result = subprocess.run(argv, text=True, capture_output=True, check=False)
    if result.returncode != 0:
        raise GateError("command_failed_" + argv[0].replace("/", "_"))
    return result.stdout


def git(argv):
    try:
        return run(["git", *argv]).strip()
    except GateError as error:
        raise GateError("git_" + str(error)) from error


MAX_HTTP_BODY = 8 * 1024 * 1024


def auth_request(token, url, limit=MAX_HTTP_BODY, allow_redirect=False):
    need(urllib.parse.urlparse(url).hostname == "api.github.com", "github_url")
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": "Bearer " + token,
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "velnor-oci-gate",
        },
    )

    class SafeRedirect(urllib.request.HTTPRedirectHandler):
        def __init__(self):
            self.cross_host = False

        def redirect_request(self, request, file, code, message, headers, new_url):
            source = urllib.parse.urlparse(request.full_url)
            target = urllib.parse.urlparse(urllib.parse.urljoin(request.full_url, new_url))
            same_host = target.hostname == source.hostname
            need(target.scheme == "https" and target.hostname, "github_redirect")
            if same_host:
                need(allow_redirect, "github_redirect")
            else:
                need(allow_redirect and not self.cross_host, "github_redirect")
                self.cross_host = True
            redirected = super().redirect_request(request, file, code, message, headers, new_url)
            if redirected is not None and not same_host:
                redirected.headers.pop("Authorization", None)
                redirected.unredirected_hdrs.pop("Authorization", None)
            return redirected

    try:
        opener = urllib.request.build_opener(SafeRedirect)
        with opener.open(request, timeout=30) as response:
            if response.status != 200:
                raise GateError("github_status")
            length = response.headers.get("Content-Length")
            if length is not None:
                need(re.fullmatch(r"[0-9]+", length) is not None, "github_length")
                need(int(length) <= limit, "github_response_bound")
            payload = response.read(limit + 1)
            need(len(payload) <= limit, "github_response_bound")
            return payload
    except (OSError, urllib.error.HTTPError, GateError) as error:
        if isinstance(error, GateError):
            raise
        raise GateError("github_request") from error


def api_json(token, url):
    return json_document(auth_request(token, url).decode("utf-8"), "github_json")


def repo_config(config):
    repository = required("REPOSITORY")
    need(repository == config.get("repository"), "repository_binding")
    return repository


def github_root(repository):
    return "https://api.github.com/repos/" + urllib.parse.quote(repository, safe="/")


def verify_producer(token, ctx):
    name = required("ARTIFACT_JOB")
    need(re.fullmatch(r"[A-Za-z0-9_. ()-]{1,200}", name) is not None, "artifact_job")
    route = github_root(ctx["repository"]) + f"/actions/runs/{ctx['run']}/attempts/{ctx['attempt']}/jobs"
    matches = []
    for page in range(1, 11):
        document = api_json(token, route + f"?per_page=100&page={page}")
        entries = document.get("jobs") if isinstance(document, dict) else None
        need(isinstance(entries, list), "artifact_jobs")
        matches.extend(job for job in entries if isinstance(job, dict) and job.get("name") == name)
        if len(entries) < 100:
            break
    else:
        raise GateError("artifact_job_pagination")
    need(len(matches) == 1, "artifact_job_ambiguous")
    job = matches[0]
    need(job.get("run_id") == ctx["run"] and job.get("run_attempt") == ctx["attempt"], "artifact_job_run")
    need(job.get("head_sha") == ctx["source"] and job.get("status") == "completed" and job.get("conclusion") == "success", "artifact_job_result")


def verify_ref(config, sha, tag):
    need(source_sha(sha), "bad_source_sha")
    need(semver(tag, tag=True), "bad_release_tag")
    tag_sha = git(["rev-parse", "refs/tags/" + tag + "^{commit}"])
    branch_sha = git(["rev-parse", "refs/remotes/origin/" + config["default_branch"]])
    need(tag_sha == sha and branch_sha == sha, "source_binding")


def verify_main(config):
    repository = repo_config(config)
    need(os.environ.get("EVENT_NAME") in {"push", "workflow_dispatch"}, "event_binding")
    ref = required("REF")
    need(ref.startswith("refs/tags/"), "release_tag_ref")
    tag = ref.removeprefix("refs/tags/")
    sha = required("SOURCE_SHA")
    need(oci_version(tag[1:]), "unsupported_oci_version")
    verify_ref(config, sha, tag)
    need(config["ci_workflow"] in {"ci.yml", ".github/workflows/ci.yml"}, "ci_identity")
    os.environ.update({
        "APPROVED_REPOSITORY": repository,
        "APPROVED_SOURCE_SHA": sha,
        "APPROVED_DEFAULT_BRANCH": config["default_branch"],
        "ADMISSION_EVENT_POLICY": "oci-tag",
    })
    try:
        shared_main()
    except RuntimeError as error:
        raise GateError(str(error).split(":", 1)[-1]) from error
    output = required("GITHUB_OUTPUT")
    with open(output, "a", encoding="utf-8") as handle:
        handle.write("version=" + tag[1:] + "\n")


def resolve_tag(token, root, tag):
    current = api_json(token, root + "/git/ref/tags/" + urllib.parse.quote(tag, safe=""))["object"]
    for _ in range(3):
        need(isinstance(current, dict), "tag_object")
        if current.get("type") == "commit":
            return current.get("sha")
        need(current.get("type") == "tag", "tag_object_type")
        current = api_json(token, root + "/git/tags/" + current.get("sha", ""))["object"]
    raise GateError("tag_chain")


def source_main(config):
    repository = repo_config(config)
    token = required("GH_TOKEN")
    sha = required("SOURCE_SHA")
    need(source_sha(sha), "bad_source_sha")
    ref = required("REF")
    need(ref.startswith("refs/tags/") and semver(ref[10:], tag=True), "release_tag_ref")
    need(oci_version(ref[11:]), "unsupported_oci_version")
    root = github_root(repository)
    branch = api_json(token, root + "/git/ref/heads/" + urllib.parse.quote(config["default_branch"], safe=""))
    need(branch.get("object", {}).get("sha") == sha, "source_branch_moved")
    need(resolve_tag(token, root, ref[10:]) == sha, "source_tag_moved")


def publish_admission_main(config):
    source_main(config)
    os.environ["APPROVED_REPOSITORY"] = repo_config(config)
    os.environ["APPROVED_SOURCE_SHA"] = required("SOURCE_SHA")
    os.environ["APPROVED_DEFAULT_BRANCH"] = config["default_branch"]
    os.environ["ADMISSION_EVENT_POLICY"] = "oci-tag"
    shared_main()


def docker_inspect(ref, raw=False):
    argv = ["docker", "buildx", "imagetools", "inspect", ref]
    if raw:
        argv.append("--raw")
    else:
        argv.extend(["--format", "{{json .Manifest}}"])
    try:
        return json_document(run(argv), "imagetools_json")
    except GateError as error:
        raise GateError("imagetools_inspect") from error


def docker_full_inspect(ref):
    try:
        return json_document(
            run(["docker", "buildx", "imagetools", "inspect", ref, "--format", "{{json .}}"]),
            "imagetools_json",
        )
    except GateError as error:
        raise GateError("imagetools_inspect") from error


def docker_field(ref, field):
    try:
        return json_document(
            run(["docker", "buildx", "imagetools", "inspect", ref, "--format", "{{json ." + field + "}}"]),
            "imagetools_json",
        )
    except GateError as error:
        raise GateError("imagetools_inspect") from error


KNOWN_MISSING = {
    "manifest unknown", "name unknown", "no such manifest",
    "manifest unknown: manifest unknown", "no such manifest: manifest unknown",
}


def tag_digest(image, version):
    ref = image + ":" + version
    result = subprocess.run(["docker", "buildx", "imagetools", "inspect", ref, "--format", "{{json .Manifest}}"], text=True, capture_output=True, check=False)
    if result.returncode != 0:
        message = " ".join(result.stderr.lower().split())
        if message.startswith("error: "):
            message = message[7:]
        if message in KNOWN_MISSING:
            return None
        raise GateError("imagetools_tag_error")
    manifest = json_document(result.stdout, "imagetools_json")
    digest = manifest.get("digest") if isinstance(manifest, dict) else None
    need(lower_sha(digest), "imagetools_digest")
    return digest


def output_values(values):
    output = required("GITHUB_OUTPUT")
    with open(output, "a", encoding="utf-8") as handle:
        for key, value in values.items():
            handle.write(key + "=" + value + "\n")


def recovery_map(raw, ids):
    value = json_document(raw, "recovery_json")
    need(isinstance(value, dict), "recovery_shape")
    need(set(value).issubset(ids), "recovery_unknown_id")
    for digest in value.values():
        need(lower_sha(digest), "recovery_digest")
    return value


def admission_main():
    image = required("IMAGE")
    image_id = required("IMAGE_ID")
    version = required("VERSION")
    need(safe_id(image_id) and oci_version(version), "image_identity")
    ids = [item for item in required("ALL_IDS").split(",") if item]
    need(len(ids) == len(set(ids)) and image_id in ids and all(safe_id(item) for item in ids), "image_ids")
    recovery = recovery_map(required("RECOVERY_JSON"), set(ids))
    digest = tag_digest(image, version)
    if digest is None:
        output_values({"existing": "false", "index_digest": ""})
        return
    need(recovery.get(image_id) == digest, "existing_without_exact_recovery")
    output_values({"existing": "true", "index_digest": digest})


def identity_env():
    image = required("IMAGE")
    image_id = required("IMAGE_ID")
    arch = required("ARCH")
    version = required("VERSION")
    sha = required("SOURCE_SHA")
    run_id = required("GITHUB_RUN_ID")
    attempt = required("GITHUB_RUN_ATTEMPT")
    need(safe_id(image_id) and safe_arch(arch) and oci_version(version) and source_sha(sha), "record_identity")
    need(re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[1-9][0-9]*", attempt), "run_identity")
    return image, image_id, arch, version, sha, run_id, attempt


def record_main():
    image, image_id, arch, version, sha, run_id, attempt = identity_env()
    digest = required("DIGEST")
    need(lower_sha(digest), "record_digest")
    identity = {"schema": 1, "image_id": image_id, "image": image, "arch": arch, "version": version, "source_sha": sha, "digest": digest, "run_id": run_id, "run_attempt": attempt}
    directory = Path("digests")
    directory.mkdir(mode=0o755, exist_ok=True)
    need(directory.is_dir() and not directory.is_symlink(), "record_directory")
    path = directory / (image_id + "-" + arch + ".json")
    need(not path.is_symlink(), "record_symlink")
    if path.exists():
        need(json_file(path, "record_json") == identity, "record_conflict")
        return
    try:
        with path.open("x", encoding="utf-8") as handle:
            json.dump(identity, handle, separators=(",", ":"), sort_keys=True)
    except OSError as error:
        raise GateError("record_write") from error
