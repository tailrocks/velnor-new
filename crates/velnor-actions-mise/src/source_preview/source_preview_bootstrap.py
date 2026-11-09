# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request

REPOSITORY = "tailrocks/velnor-new"
REPOSITORY_ID = 1390620900
API = "api.github.com"
RELEASE_HOSTS = {"github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"}
TARGET = "x86_64-unknown-linux-gnu"
MAX_LOCK = 1_048_576
MAX_JSON = 2_097_152
MAX_RECEIPT = 65_536
MAX_BUNDLE = 4_194_304
MAX_PAYLOAD = 268_435_456
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")


class AdmissionError(Exception):
    pass


def require(condition):
    if not condition:
        raise AdmissionError("source_preview_admission_rejected")


def sha40(value):
    return isinstance(value, str) and HEX40.fullmatch(value) is not None


def hex64(value):
    return isinstance(value, str) and HEX64.fullmatch(value) is not None


def number(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def parse_json(data, limit):
    require(len(data) <= limit)
    try:
        return json.loads(data, object_pairs_hook=unique_pairs)
    except (ValueError, UnicodeError) as error:
        raise AdmissionError("source_preview_invalid_json") from error


def parse_lock(data):
    require(len(data) <= MAX_LOCK)
    try:
        return tomllib.loads(data.decode("utf-8"))
    except (ValueError, UnicodeError) as error:
        raise AdmissionError("source_preview_invalid_lock") from error


def read_regular(path, limit):
    path = Path(path)
    require(path.is_absolute())
    for parent in (path, *path.parents):
        metadata = os.lstat(parent)
        require(not stat.S_ISLNK(metadata.st_mode))
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        require(stat.S_ISREG(os.fstat(fd).st_mode))
        with os.fdopen(fd, "rb", closefd=False) as file:
            data = file.read(limit + 1)
        require(len(data) <= limit)
        return data
    finally:
        os.close(fd)


def workflow_tuple(record, prefix, path, source, git_ref):
    require(record[f"{prefix}_workflow_path"] == path)
    require(record[f"{prefix}_workflow_ref"] == f"{REPOSITORY}/{path}@{git_ref}")
    require(record[f"{prefix}_workflow_sha"] == source)
    require(number(record[f"{prefix}_run_id"]) and number(record[f"{prefix}_run_attempt"]))


def validate_preview(record, candidate):
    require(isinstance(record, dict))
    fixed = {"schema", "repository", "version", "source_sha", "tag", "release_id", "receipt_sha256", "bundle_sha256", "candidate_source_artifact_id", "publisher_run_attempt", "publisher_job_id"}
    runs = {f"{prefix}_{suffix}" for prefix in ("source", "draft", "signer", "ci", "qualification") for suffix in ("workflow_path", "workflow_ref", "workflow_sha", "run_id", "run_attempt")}
    target_fields = {f"{prefix}_{suffix}" for prefix in ("linux", "darwin") for suffix in ("producer_run_id", "producer_run_attempt", "producer_payload_artifact_id", "producer_payload_archive_sha256", "candidate_payload_artifact_id", "qualification_job_id", "payload_target", "payload_name", "payload_size", "payload_sha256")}
    require(set(record) == fixed | runs | target_fields)
    source = record["source_sha"]
    require(type(record["schema"]) is int and record["schema"] == 3 and record["repository"] == REPOSITORY)
    require(isinstance(record["version"], str) and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", record["version"]) is not None)
    require(sha40(source) and sha40(candidate) and source != candidate)
    require(record["tag"] == "sdk-preview-" + source and number(record["release_id"]))
    for role in ("source", "draft"):
        workflow_tuple(record, role, ".github/workflows/product-release-generator.yml", source, "refs/tags/" + record["tag"])
        require(record[role + "_run_id"] == record["signer_run_id"])
    require(all(number(record[key]) for key in ("candidate_source_artifact_id", "publisher_run_attempt", "publisher_job_id")))
    artifact_ids = [record["candidate_source_artifact_id"]]
    job_ids = []
    for prefix, target in (("linux", TARGET), ("darwin", "aarch64-apple-darwin")):
        require(all(number(record[f"{prefix}_{key}"]) for key in ("producer_run_id", "producer_run_attempt", "producer_payload_artifact_id", "candidate_payload_artifact_id", "qualification_job_id", "payload_size")))
        require(record[f"{prefix}_payload_size"] <= MAX_PAYLOAD)
        require(record[f"{prefix}_payload_target"] == target and record[f"{prefix}_payload_name"] == f"velnor-actions-{target}")
        require(all(hex64(record[f"{prefix}_{key}"]) for key in ("producer_payload_archive_sha256", "payload_sha256")))
        artifact_ids.extend((record[f"{prefix}_producer_payload_artifact_id"], record[f"{prefix}_candidate_payload_artifact_id"]))
        job_ids.append(record[f"{prefix}_qualification_job_id"])
    require(len(set(artifact_ids)) == len(artifact_ids) and len(set(job_ids)) == 2)
    require(record["source_run_attempt"] <= record["draft_run_attempt"] <= record["signer_run_attempt"] <= record["publisher_run_attempt"] <= 51)
    workflow_tuple(record, "signer", ".github/workflows/product-release-generator.yml", source, "refs/tags/" + record["tag"])
    workflow_tuple(record, "ci", ".github/workflows/ci.yml", source, "refs/heads/main")
    workflow_tuple(record, "qualification", ".github/workflows/source-qualification.yml", source, "refs/heads/main")
    require(all(hex64(record[key]) for key in ("receipt_sha256", "bundle_sha256")))


def event_binding(context, network):
    event = context["event"]
    require(isinstance(event, dict) and context["repository"] == REPOSITORY)
    live_repository = network.api(f"/repos/{REPOSITORY}")
    require(live_repository.get("id") == REPOSITORY_ID and live_repository.get("full_name") == REPOSITORY)
    require(live_repository.get("default_branch") == "main")
    repository = event.get("repository", {})
    require(repository.get("full_name") == REPOSITORY and repository.get("id") == REPOSITORY_ID)
    candidate = context["candidate"]
    require(sha40(candidate))
    kind = context["event_name"]
    if kind == "pull_request":
        pr = event.get("pull_request", {})
        base = pr.get("base", {})
        head = pr.get("head", {})
        require(number(event.get("number")) and base.get("ref") == "main")
        require(base.get("repo", {}).get("id") == REPOSITORY_ID)
        require(sha40(base.get("sha")) and sha40(head.get("sha")))
        require(number(head.get("repo", {}).get("id")))
        live = network.api(f"/repos/{REPOSITORY}/pulls/{event['number']}")
        require(live.get("number") == event["number"] and live.get("state") == "open")
        require(live.get("base", {}).get("sha") == base["sha"] and live.get("base", {}).get("ref") == "main")
        require(live.get("base", {}).get("repo", {}).get("id") == REPOSITORY_ID)
        require(live.get("head", {}).get("sha") == head["sha"])
        require(live.get("head", {}).get("repo", {}).get("id") == head.get("repo", {}).get("id"))
        base_sha = base["sha"]
    elif kind == "merge_group":
        group = event.get("merge_group", {})
        base_sha = group.get("base_sha")
        require(sha40(base_sha) and group.get("base_ref") == "refs/heads/main")
        require(group.get("head_sha") == candidate and group.get("head_ref") == context["ref"])
    elif kind == "push":
        base_sha = event.get("before")
        require(sha40(base_sha) and context["ref"] == "refs/heads/main")
        require(event.get("after") == candidate and event.get("deleted") is False)
    elif kind == "workflow_dispatch":
        require(context["ref"] == "refs/heads/main")
        commit = network.api(f"/repos/{REPOSITORY}/commits/{candidate}")
        parents = commit.get("parents")
        require(commit.get("sha") == candidate and isinstance(parents, list) and len(parents) > 0)
        base_sha = parents[0].get("sha")
        require(sha40(base_sha))
    else:
        raise AdmissionError("source_preview_unsupported_event")
    live_main = network.api(f"/repos/{REPOSITORY}/commits/main")
    expected_main = base_sha if kind in ("pull_request", "merge_group") else candidate
    require(live_main.get("sha") == expected_main)
    return base_sha


def read_github_lock(network, commit):
    require(sha40(commit))
    blob = network.api(f"/repos/{REPOSITORY}/contents/.velnor/generator.lock?ref={commit}")
    require(blob.get("type") == "file" and blob.get("encoding") == "base64")
    require(blob.get("path") == ".velnor/generator.lock" and number(blob.get("size")) and blob["size"] <= MAX_LOCK)
    try:
        raw = base64.b64decode("".join(blob["content"].split()), validate=True)
    except (ValueError, KeyError) as error:
        raise AdmissionError("source_preview_invalid_base_lock") from error
    require(len(raw) == blob["size"])
    return parse_lock(raw)


def approved_preview(candidate_lock, base_lock, role, candidate):
    require(role in ("planner", "runtime"))
    active_key = f"{role}-preview"
    candidate_preview = candidate_lock.get(active_key)
    base_active = base_lock.get(active_key)
    if candidate_preview is None:
        require(base_active is None)
        return None
    validate_preview(candidate_preview, candidate)
    approved = (base_active, base_lock.get(f"{role}-preview-staged"))
    require(any(value == candidate_preview for value in approved if value is not None))
    return candidate_preview


def approved_controller(context, network):
    base_sha = event_binding(context, network)
    candidate_lock = parse_lock(read_regular(context["lock_path"], MAX_LOCK))
    base_lock = read_github_lock(network, base_sha)
    preview = approved_preview(candidate_lock, base_lock, "planner", context["candidate"])
    return preview, base_lock, base_sha


def stable_identity(base_lock):
    generator = base_lock.get("generator")
    require(isinstance(generator, dict) and generator.get("binary") == "velnor-actions")
    version = generator.get("version")
    require(isinstance(version, str) and re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version) is not None)
    require(sha40(generator.get("commit")))
    binaries = generator.get("binaries")
    require(isinstance(binaries, list))
    matches = [entry for entry in binaries if isinstance(entry, dict) and entry.get("target") == TARGET]
    require(len(matches) == 1)
    item = matches[0]
    expected = f"https://github.com/{REPOSITORY}/releases/download/v{version}/velnor-actions-{version}-{TARGET}"
    require(item.get("artifact") == expected and hex64(item.get("sha256")))
    return {"channel": "stable", "version": version, "target": TARGET, "sha256": item["sha256"]}, expected


def receipt_from_record(record):
    def run(prefix):
        return {suffix: record[f"{prefix}_{suffix}"] for suffix in ("workflow_path", "workflow_ref", "workflow_sha", "run_id", "run_attempt")}
    return {
        "schema": record["schema"],
        "repository": record["repository"],
        "version": record["version"],
        "source_sha": record["source_sha"],
        "tag": record["tag"],
        "release_id": record["release_id"],
        "source": run("source"),
        "draft": run("draft"),
        "signer": run("signer"),
        "ci": run("ci"),
        "qualification": run("qualification"),
        "candidate_source_artifact_id": record["candidate_source_artifact_id"],
        "targets": {prefix: {
            "producer": {key: record[f"{prefix}_{key if key.startswith('producer_') else 'producer_' + key}"] for key in ("producer_run_id", "producer_run_attempt", "payload_artifact_id", "payload_archive_sha256")},
            "candidate_payload_artifact_id": record[f"{prefix}_candidate_payload_artifact_id"],
            "qualification_job_id": record[f"{prefix}_qualification_job_id"],
            "payload": {key: record[f"{prefix}_payload_{key}"] for key in ("target", "name", "size", "sha256")},
        } for prefix in ("linux", "darwin")},
    }


def checked_asset(network, url, expected_size, expected_digest, maximum):
    require(number(expected_size) and expected_size <= maximum and hex64(expected_digest))
    body = network.asset(url, maximum)
    require(len(body) == expected_size and hashlib.sha256(body).hexdigest() == expected_digest)
    return body


def asset_metadata(release, name, maximum, url):
    assets = release.get("assets")
    require(isinstance(assets, list))
    matches = [item for item in assets if isinstance(item, dict) and item.get("name") == name]
    require(len(matches) == 1)
    entry = matches[0]
    require(entry.get("state") == "uploaded" and number(entry.get("size")) and entry["size"] <= maximum)
    require(entry.get("browser_download_url") == url)
    digest = entry.get("digest")
    require(isinstance(digest, str) and digest.startswith("sha256:") and hex64(digest[7:]))
    return entry["size"], digest[7:]


def verify_annotated_tag(preview, network):
    reference = network.api(f"/repos/{REPOSITORY}/git/ref/tags/{preview['tag']}")
    require(reference.get("ref") == "refs/tags/" + preview["tag"])
    tag = reference.get("object", {})
    require(tag.get("type") == "tag" and sha40(tag.get("sha")))
    annotated = network.api(f"/repos/{REPOSITORY}/git/tags/{tag['sha']}")
    require(annotated.get("sha") == tag["sha"] and annotated.get("tag") == preview["tag"])
    target = annotated.get("object", {})
    require(target.get("type") == "commit" and target.get("sha") == preview["source_sha"])
    return tag["sha"]


def download_verified_release(preview, network):
    release = network.api(f"/repos/{REPOSITORY}/releases/{preview['release_id']}")
    require(release.get("id") == preview["release_id"] and release.get("tag_name") == preview["tag"])
    verify_annotated_tag(preview, network)
    require(release.get("draft") is False and release.get("prerelease") is True and release.get("immutable") is True)
    names = ("SDK_PREVIEW_RECEIPT.json", "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl", preview["linux_payload_name"], preview["darwin_payload_name"])
    assets = release.get("assets")
    require(isinstance(assets, list) and len(assets) == len(names))
    require(sorted(item.get("name") for item in assets if isinstance(item, dict)) == sorted(names))
    prefix = f"https://github.com/{REPOSITORY}/releases/download/{preview['tag']}/"
    receipt_size, receipt_digest = asset_metadata(release, names[0], MAX_RECEIPT, prefix + names[0])
    bundle_size, bundle_digest = asset_metadata(release, names[1], MAX_BUNDLE, prefix + names[1])
    require(receipt_digest == preview["receipt_sha256"] and bundle_digest == preview["bundle_sha256"])
    receipt = checked_asset(network, prefix + names[0], receipt_size, receipt_digest, MAX_RECEIPT)
    bundle = checked_asset(network, prefix + names[1], bundle_size, bundle_digest, MAX_BUNDLE)
    payloads = {}
    for target in ("linux", "darwin"):
        name = preview[f"{target}_payload_name"]
        size, digest = asset_metadata(release, name, MAX_PAYLOAD, prefix + name)
        require(size == preview[f"{target}_payload_size"] and digest == preview[f"{target}_payload_sha256"])
        payloads[target] = checked_asset(network, prefix + name, size, digest, MAX_PAYLOAD)
    parsed = parse_json(receipt, MAX_RECEIPT)
    require(parsed == receipt_from_record(preview))
    require(json.dumps(parsed, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8") == receipt)
    return receipt, bundle, payloads


def verify_checkout(context):
    workspace = Path(context["workspace"])
    require(workspace.is_absolute() and not workspace.is_symlink())
    require(Path(context["lock_path"]) == workspace / ".velnor" / "generator.lock")
    git = subprocess.run(
        ["/usr/bin/git", "-C", str(workspace), "rev-parse", "--verify", "HEAD^{commit}"],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        env={"PATH": "/usr/bin:/bin", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"},
        timeout=15, check=False,
    )
    require(git.returncode == 0 and git.stdout.decode("ascii").strip() == context["candidate"])


def preview_identity(preview):
    return {"channel": "source-preview", "version": preview["version"], "target": TARGET, "sha256": preview["linux_payload_sha256"], "source_sha": preview["source_sha"], "receipt_sha256": preview["receipt_sha256"]}


def authorize_and_acquire(context, network, required_identity=None):
    verify_checkout(context)
    preview, base_lock, base = approved_controller(context, network)
    if preview is None:
        tag_sha = None
        identity, stable_url = stable_identity(base_lock)
        require(required_identity is None)
        payload = network.asset(stable_url, MAX_PAYLOAD)
        require(0 < len(payload) <= MAX_PAYLOAD and hashlib.sha256(payload).hexdigest() == identity["sha256"])
    else:
        bootstrap_identity = preview_identity(preview)
        identity = bootstrap_identity
        require(required_identity is None or required_identity == bootstrap_identity)
        tag_sha = verify_annotated_tag(preview, network)
        _, _, payloads = download_verified_release(preview, network)
        payload = payloads["linux"]
    require(event_binding(context, network) == base)
    runner_temp = Path(context["runner_temp"])
    require(runner_temp.is_absolute())
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    root_fd = os.open(runner_temp, flags)
    directory_fd = root_fd
    try:
        check_owned_directory(root_fd)
        for name in ("velnor", "bin"):
            try:
                os.mkdir(name, mode=0o700, dir_fd=directory_fd)
            except FileExistsError:
                pass
            child_fd = os.open(name, flags, dir_fd=directory_fd)
            check_owned_directory(child_fd)
            if directory_fd != root_fd:
                os.close(directory_fd)
            directory_fd = child_fd
        temporary = ".controller-" + secrets.token_hex(16)
        os.mkdir(temporary, mode=0o700, dir_fd=directory_fd)
        temporary_fd = os.open(temporary, flags, dir_fd=directory_fd)
        try:
            for name, contents, mode in (
                ("binary", payload, 0o700),
                ("identity.json", json.dumps(identity, sort_keys=True, separators=(",", ":")).encode(), 0o600),
            ):
                fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode, dir_fd=temporary_fd)
                with os.fdopen(fd, "wb") as file:
                    file.write(contents)
                    file.flush()
                    os.fsync(file.fileno())
            os.fsync(temporary_fd)
            require(event_binding(context, network) == base)
            if preview is not None:
                require(verify_annotated_tag(preview, network) == tag_sha)
            rename_exclusive(directory_fd, temporary, "controller")
        except BaseException:
            for name in ("binary", "identity.json"):
                try:
                    os.unlink(name, dir_fd=temporary_fd)
                except FileNotFoundError:
                    pass
            os.rmdir(temporary, dir_fd=directory_fd)
            raise
        finally:
            os.close(temporary_fd)
    finally:
        if directory_fd != root_fd:
            os.close(directory_fd)
        os.close(root_fd)
    return str(runner_temp / "velnor" / "bin" / "controller" / "binary")


def rename_exclusive(directory_fd, source, target):
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "linux":
        operation = libc.renameat2
        flag = 1
    elif sys.platform == "darwin":
        operation = libc.renameatx_np
        flag = 4
    else:
        raise AdmissionError("source_preview_unsupported_platform")
    operation.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    operation.restype = ctypes.c_int
    if operation(directory_fd, os.fsencode(source), directory_fd, os.fsencode(target), flag) != 0:
        raise OSError(ctypes.get_errno(), "source_preview_controller_rename")


def verify_controller(context, network):
    verify_checkout(context)
    preview, base_lock, base = approved_controller(context, network)
    if preview is None:
        tag_sha = None
        expected, _ = stable_identity(base_lock)
    else:
        tag_sha = verify_annotated_tag(preview, network)
        expected = {"channel": "source-preview", "version": preview["version"], "target": TARGET, "sha256": preview["linux_payload_sha256"], "source_sha": preview["source_sha"], "receipt_sha256": preview["receipt_sha256"]}
    root = Path(context["runner_temp"]) / "velnor" / "bin" / "controller"
    raw = read_regular(root / "identity.json", MAX_RECEIPT)
    actual = parse_json(raw, MAX_RECEIPT)
    require(raw == json.dumps(actual, sort_keys=True, separators=(",", ":")).encode() and actual == expected)
    binary = read_regular(root / "binary", MAX_PAYLOAD)
    require(hashlib.sha256(binary).hexdigest() == expected["sha256"])
    require(event_binding(context, network) == base)
    if preview is not None:
        require(verify_annotated_tag(preview, network) == tag_sha)
    return expected


def check_owned_directory(fd):
    info = os.fstat(fd)
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.geteuid() and info.st_mode & 0o022 == 0)


class FixedRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        old = urllib.parse.urlsplit(request.full_url)
        new = urllib.parse.urlsplit(new_url)
        require(old.hostname != API and new.scheme == "https" and new.hostname in RELEASE_HOSTS)
        require(new.port in (None, 443) and new.username is None and new.password is None)
        redirected = super().redirect_request(request, file, code, message, headers, new_url)
        require(redirected is not None)
        for key in list(redirected.headers):
            if key.lower() in ("authorization", "cookie", "proxy-authorization"):
                redirected.remove_header(key)
        return redirected


class GitHubTransport:
    def __init__(self, token):
        import ssl
        self.token = token
        self.deadline = time.monotonic() + 180
        ca_file = "/etc/ssl/cert.pem" if sys.platform == "darwin" else "/etc/ssl/certs/ca-certificates.crt"
        tls = ssl.create_default_context(cafile=ca_file)
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=tls), FixedRedirect()
        )

    def get(self, url, maximum, api):
        remaining = self.deadline - time.monotonic()
        require(remaining > 0)
        parsed = urllib.parse.urlsplit(url)
        require(parsed.scheme == "https" and parsed.port in (None, 443))
        require(parsed.username is None and parsed.password is None and not parsed.fragment)
        require(parsed.hostname == (API if api else "github.com"))
        headers = {"Accept": "application/vnd.github+json", "User-Agent": "velnor-source-preview"}
        if api and self.token:
            headers["Authorization"] = "Bearer " + self.token
        request = urllib.request.Request(url, headers=headers, method="GET")
        try:
            with self.opener.open(request, timeout=min(remaining, 30)) as response:
                length = response.headers.get("Content-Length")
                if length is not None:
                    require(length.isdecimal() and int(length) <= maximum)
                result = bytearray()
                size = 0
                while True:
                    require(time.monotonic() <= self.deadline)
                    chunk = response.read1(min(65_536, maximum + 1 - size))
                    if not chunk:
                        break
                    size += len(chunk)
                    require(size <= maximum)
                    result.extend(chunk)
        except (OSError, urllib.error.URLError) as error:
            raise AdmissionError("source_preview_network_failure") from error
        require(len(result) <= maximum and time.monotonic() <= self.deadline)
        return result

    def api(self, path):
        require(path == f"/repos/{REPOSITORY}" or path.startswith(f"/repos/{REPOSITORY}/"))
        return parse_json(self.get("https://" + API + path, MAX_JSON, True), MAX_JSON)

    def asset(self, url, maximum):
        return self.get(url, maximum, False)


def environment_context():
    event_path = Path(os.environ["GITHUB_EVENT_PATH"])
    event = parse_json(read_regular(event_path, MAX_JSON), MAX_JSON)
    workspace = Path(os.environ["GITHUB_WORKSPACE"])
    context = {
        "event": event,
        "event_name": os.environ["GITHUB_EVENT_NAME"],
        "repository": os.environ["GITHUB_REPOSITORY"],
        "candidate": os.environ["GITHUB_SHA"],
        "ref": os.environ["GITHUB_REF"],
        "workspace": str(workspace),
        "lock_path": str(workspace / ".velnor" / "generator.lock"),
        "runner_temp": os.environ["RUNNER_TEMP"],
    }
    return context


def main():
    context = environment_context()
    authorize_and_acquire(context, GitHubTransport(os.environ.get("GH_TOKEN", "")))


if __name__ == "__main__":
    try:
        main()
    except (AdmissionError, OSError, KeyError, ValueError, subprocess.SubprocessError):
        raise SystemExit("source preview acquisition rejected") from None
