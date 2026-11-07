# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
from datetime import datetime, timezone
import hashlib
import http.client
import json
import os
from pathlib import Path
import platform
import re
import signal
import ssl
import stat
import subprocess
import sys
import tempfile
import time

REPOSITORY = "tailrocks/velnor-new"
GH_SHA256 = {
    ("Linux", "x86_64"): {"cbfdc94f3a5c0713c0d6df3e0a1d88a59a76b0c52332aad7d21d112f9da2cc0e", "b768184570b12536be4ac1528893f6340d1c6a7b40b56ad45bca9cd0e87ad34d"},
    ("Darwin", "arm64"): {"5e4470bb57bf1e7cb5063be940d5dffc5be3e1899d6bf7fe1ff15372f7f4cc29", "5f8d7527e2bdf584249b030cdd1550c91d75e8afb40aa3c11573d416817a1260"},
}


def require(condition):
    if not condition:
        raise ValueError("SDK preview tag precheck rejected")


def unique(pairs):
    result = {}
    for name, value in pairs:
        require(name not in result)
        result[name] = value
    return result


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def now_utc():
    return datetime.now(timezone.utc)


class GitHub:
    def __init__(self, token):
        self.token = token
        self.deadline = time.monotonic() + 180

    def request(self, method, path, body=None, allowed=(200,)):
        require(path == "/user" or path == "/users/donbeave" or path.startswith("/repos/" + REPOSITORY + "/"))
        remaining = self.deadline - time.monotonic()
        require(remaining > 0)
        connection = http.client.HTTPSConnection("api.github.com", timeout=min(30, remaining), context=ssl.create_default_context())
        try:
            headers = {"Accept": "application/vnd.github+json", "Authorization": "Bearer " + self.token, "User-Agent": "velnor-sdk-preview-tag", "X-GitHub-Api-Version": "2022-11-28"}
            if body is not None:
                headers["Content-Type"] = "application/json"
            connection.request(method, path, body=body, headers=headers)
            response = connection.getresponse()
            require(response.status in allowed)
            if response.status == 404:
                return None
            raw = response.read(2097153)
            require(len(raw) <= 2097152 and time.monotonic() < self.deadline)
            return json.loads(raw, object_pairs_hook=unique)
        finally:
            connection.close()

    def get(self, path):
        return self.request("GET", path)


def verified_gh(path, target):
    require(os.path.isabs(path))
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(descriptor)
        require(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= 104857600 and before.st_mode & 0o111)
        digest = hashlib.sha256()
        size = 0
        with target.open("xb") as output:
            while True:
                chunk = os.read(descriptor, 65536)
                if not chunk:
                    break
                size += len(chunk)
                require(size <= 104857600)
                digest.update(chunk)
                output.write(chunk)
        after = os.fstat(descriptor)
        require(size == before.st_size and (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns))
        require(digest.hexdigest() in GH_SHA256.get((platform.system(), platform.machine()), set()))
    finally:
        os.close(descriptor)
    target.chmod(0o500)


def authority(client, trusted_revision):
    root = "/repos/" + REPOSITORY + "/contents/crates/velnor-actions-workflow-renderer/src/"
    namespace = {"__name__": "protected_source_authority"}
    for name in ("schema2_generator_source_preview_artifact.py", "schema2_generator_source_preview_tag.py", "schema2_generator_source_preview_source.py"):
        value = client.get(root + name + "?ref=" + trusted_revision)
        require(value.get("path") == "crates/velnor-actions-workflow-renderer/src/" + name and value.get("encoding") == "base64")
        code = base64.b64decode(value["content"], validate=False)
        require(len(code) <= 1048576)
        exec(compile(code, name, "exec"), namespace)
    return namespace


def absent(client, path):
    require(client.request("GET", path, allowed=(200, 404)) is None)


def check_main(client, source):
    value = client.get("/repos/" + REPOSITORY + "/git/ref/heads/main")
    require(value.get("object", {}).get("type") == "commit" and value.get("object", {}).get("sha") == source)


def create(source, gh, trusted_revision):
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("SDK preview tag deadline")))
    signal.setitimer(signal.ITIMER_REAL, 300)
    require(re.fullmatch("[0-9a-f]{40}", source) is not None)
    require(re.fullmatch("[0-9a-f]{40}", trusted_revision) is not None and trusted_revision != source)
    with tempfile.TemporaryDirectory(prefix="velnor-preview-tag-") as temporary:
        executable = Path(temporary).resolve() / "gh"
        verified_gh(gh, executable)
        environment = {"PATH": "/usr/bin:/bin", "HOME": os.environ["HOME"], "GH_PROMPT_DISABLED": "1"}
        if "GH_CONFIG_DIR" in os.environ:
            environment["GH_CONFIG_DIR"] = os.environ["GH_CONFIG_DIR"]
        token = subprocess.run([str(executable), "auth", "token"], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=15, check=True, env=environment).stdout
        require(0 < len(token) < 4096 and token.endswith(b"\n") and b"\n" not in token[:-1])
        client = GitHub(token[:-1].decode("utf-8"))
        user = client.get("/user")
        require(user.get("id") == 139017 and user.get("login") == "donbeave")
        check_main(client, source)
        ancestry = client.get("/repos/" + REPOSITORY + "/compare/" + trusted_revision + "..." + source)
        require(ancestry.get("status") == "ahead" and ancestry.get("merge_base_commit", {}).get("sha") == trusted_revision)
        gate = authority(client, trusted_revision)
        gate["verify_tag_rules"](client)
        gate["verify_default_ancestry"](client, source)
        version = gate["source_version"](client, source)
        proof = gate["producer_inputs"](client, source)
        require(set(proof) == {"producer_run_id", "producer_run_attempt", "payload_artifact_id", "payload_archive_sha256"})
        artifact = gate["producer_artifact"](client, source, proof["producer_run_id"], proof["producer_run_attempt"])
        require(artifact.get("id") == proof["payload_artifact_id"] and artifact.get("digest") == "sha256:" + proof["payload_archive_sha256"])
        gate["download_verified_payload"](client, artifact, Path(temporary).resolve() / "producer-payload")
        tag = "sdk-preview-" + source
        reference = "/repos/" + REPOSITORY + "/git/ref/tags/" + tag
        absent(client, reference)
        require(client.get("/repos/" + REPOSITORY + "/immutable-releases").get("enabled") is True)
        checked = now_utc().strftime("%Y-%m-%dT%H:%M:%SZ")
        record = {"schema": 1, "repository": REPOSITORY, "artifact": "generator", "source_sha": source, "version": version, **proof, "immutability": {"enabled": True, "checked_at": checked}}
        message = canonical(record).decode("utf-8")
        gate["annotation"](message, source, checked, now_utc(), version)
        body = canonical({"tag": tag, "message": message, "object": source, "type": "commit", "tagger": {"name": "Alexey Zhokhov", "email": "alexey@zhokhov.com", "date": checked}})
        created = client.request("POST", "/repos/" + REPOSITORY + "/git/tags", body, allowed=(201,))
        oid = created.get("sha")
        require(isinstance(oid, str) and re.fullmatch("[0-9a-f]{40}", oid) and created.get("tag") == tag and created.get("object", {}).get("type") == "commit" and created.get("object", {}).get("sha") == source)
        check_main(client, source)
        gate["verify_tag_rules"](client)
        latest = gate["producer_artifact"](client, source, proof["producer_run_id"], proof["producer_run_attempt"])
        require(latest.get("id") == artifact["id"] and latest.get("digest") == artifact["digest"] and latest.get("size_in_bytes") == artifact["size_in_bytes"])
        absent(client, reference)
        require(client.get("/repos/" + REPOSITORY + "/immutable-releases").get("enabled") is True)
        current = now_utc()
        require(0 <= (current - datetime.strptime(checked, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)).total_seconds() <= 60)
        gate["annotation"](message, source, checked, current, version)
        result = client.request("POST", "/repos/" + REPOSITORY + "/git/refs", canonical({"ref": "refs/tags/" + tag, "sha": oid}), allowed=(201,))
        require(result.get("ref") == "refs/tags/" + tag and result.get("object", {}).get("type") == "tag" and result.get("object", {}).get("sha") == oid)
        print(tag)


if __name__ == "__main__":
    try:
        require(len(sys.argv) == 5 and sys.argv[1] == "create")
        create(sys.argv[2], sys.argv[3], sys.argv[4])
    except (OSError, ValueError, KeyError, TypeError, TimeoutError, http.client.HTTPException, subprocess.SubprocessError):
        print("SDK preview tag precheck failed", file=sys.stderr)
        sys.exit(1)
