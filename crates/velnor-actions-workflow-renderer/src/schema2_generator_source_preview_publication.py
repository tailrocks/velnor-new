# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import signal
import ssl
import stat
import sys
import time

REPOSITORY = "tailrocks/velnor-new"
WORKFLOW = ".github/workflows/product-release-generator.yml"
PAYLOAD = "velnor-actions-x86_64-unknown-linux-gnu"
RECEIPT = "SDK_PREVIEW_RECEIPT.json"
BUNDLE = RECEIPT + ".sigstore.jsonl"
MAX_PAYLOAD = 268435456
MAX_JSON = 2097152



def require(condition):
    if not condition:
        raise ValueError("preview publication rejected")


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def read_file(directory, name, limit):
    directory = Path(directory)
    require(not directory.is_symlink() and directory.is_dir())
    require({item.name for item in directory.iterdir()} == {name})
    descriptor = os.open(directory / name, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and 0 < info.st_size <= limit)
        data = bytearray()
        while len(data) <= limit:
            chunk = os.read(descriptor, min(65536, limit + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        after = os.fstat(descriptor)
        require(len(data) == info.st_size and (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns))
        return bytes(data)
    finally:
        os.close(descriptor)


def parse(data):
    return json.loads(data, object_pairs_hook=unique)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def positive(value):
    require(type(value) is int and 0 < value <= 9007199254740991)
    return value


def metadata(data):
    value = parse(data)
    source = os.environ["GITHUB_SHA"]
    require(canonical(value) == data and re.fullmatch("[0-9a-f]{40}", source))
    tag = "sdk-preview-" + source
    tag_ref = "refs/tags/" + tag
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_EVENT_NAME") == "push" and os.environ.get("GITHUB_REF") == tag_ref)
    require(os.environ.get("GITHUB_WORKFLOW_REF") == REPOSITORY + "/" + WORKFLOW + "@" + tag_ref and os.environ.get("GITHUB_WORKFLOW_SHA") == source)
    require(set(value) == {"schema", "repository", "version", "source_sha", "tag", "signer", "ci", "qualification"})
    require(type(value["schema"]) is int and value["schema"] == 1 and value["repository"] == REPOSITORY and value["source_sha"] == source and value["tag"] == tag and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value["version"]))
    for key, path in (("signer", WORKFLOW), ("ci", ".github/workflows/ci.yml"), ("qualification", ".github/workflows/source-qualification.yml")):
        run = value[key]
        require(set(run) == {"workflow_path", "workflow_ref", "workflow_sha", "run_id", "run_attempt"})
        ref = tag_ref if key == "signer" else "refs/heads/main"
        require(run["workflow_path"] == path and run["workflow_ref"] == REPOSITORY + "/" + path + "@" + ref and run["workflow_sha"] == source)
        positive(run["run_id"]); positive(run["run_attempt"])
    require(value["signer"]["run_id"] == positive(int(os.environ["GITHUB_RUN_ID"])) and value["signer"]["run_attempt"] == positive(int(os.environ["GITHUB_RUN_ATTEMPT"])))
    return value


class GitHub:
    def __init__(self):
        self.token = os.environ["GH_TOKEN"]
        self.deadline = time.monotonic() + 180

    def request(self, method, host, path, data=None, content_type="application/json", allowed=(200,)):
        require(host in ("api.github.com", "uploads.github.com") and path.startswith("/repos/" + REPOSITORY + "/"))
        remaining = self.deadline - time.monotonic()
        require(remaining > 0)
        connection = http.client.HTTPSConnection(host, timeout=min(30, remaining), context=ssl.create_default_context())
        try:
            headers = {"Accept": "application/vnd.github+json", "Authorization": "Bearer " + self.token, "User-Agent": "velnor-source-preview", "X-GitHub-Api-Version": "2022-11-28"}
            if data is not None:
                headers["Content-Type"] = content_type
                headers["Content-Length"] = str(len(data))
            connection.request(method, path, body=data, headers=headers)
            response = connection.getresponse()
            require(response.status in allowed)
            if response.status == 404:
                return None
            body = response.read(MAX_JSON + 1)
            require(len(body) <= MAX_JSON and time.monotonic() < self.deadline)
            return parse(body)
        finally:
            connection.close()

    def api(self, method, suffix, data=None, allowed=(200,)):
        return self.request(method, "api.github.com", "/repos/" + REPOSITORY + suffix, data, allowed=allowed)



def release(client, receipt, require_draft=True):
    item = client.api("GET", "/releases/" + str(positive(receipt["release_id"])))
    marker = "velnor-source-preview:" + str(receipt["signer"]["run_id"]) + ":" + str(receipt["signer"]["run_attempt"]) + ":" + receipt["source_sha"]
    require(item.get("id") == receipt["release_id"] and item.get("tag_name") == receipt["tag"] and item.get("body") == marker and item.get("prerelease") is True)
    require(item.get("draft") is require_draft)
    return item


def existing_assets(item, expected, allow_unknown_bundle=False):
    assets = item.get("assets")
    require(isinstance(assets, list) and len(assets) <= 3)
    result = set()
    for asset in assets:
        name = asset.get("name")
        require(name in (RECEIPT, BUNDLE, PAYLOAD) and name not in result and asset.get("state") == "uploaded")
        if name in expected:
            data = expected[name]
            require(asset.get("size") == len(data) and asset.get("digest") == "sha256:" + digest(data))
        else:
            require(name == BUNDLE and allow_unknown_bundle)
        result.add(name)
    return result


def verify_tag(client, source):
    tag = "sdk-preview-" + source
    reference = client.api("GET", "/git/ref/tags/" + tag)
    obj = reference.get("object", {})
    oid = obj.get("sha")
    require(obj.get("type") == "tag" and isinstance(oid, str) and re.fullmatch("[0-9a-f]{40}", oid))
    annotated = client.api("GET", "/git/tags/" + oid)
    require(annotated.get("tag") == tag and annotated.get("object", {}).get("type") == "commit" and annotated.get("object", {}).get("sha") == source)
    main = client.api("GET", "/git/ref/heads/main").get("object", {})
    require(main.get("type") == "commit" and isinstance(main.get("sha"), str) and re.fullmatch("[0-9a-f]{40}", main["sha"]))
    comparison = client.api("GET", "/compare/" + source + "..." + main["sha"])
    require(comparison.get("status") in ("ahead", "identical") and comparison.get("merge_base_commit", {}).get("sha") == source)


def main():
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("preview publication deadline")))
    signal.setitimer(signal.ITIMER_REAL, 180)
    source = metadata(read_file("source-preview-metadata", "source-preview-metadata.json", 65536))
    payload = read_file("source-preview-payload", PAYLOAD, MAX_PAYLOAD)
    client = GitHub()
    verify_tag(client, source["source_sha"])
    mode = os.environ["PREVIEW_MODE"]
    if mode == "draft":
        marker = "velnor-source-preview:" + str(source["signer"]["run_id"]) + ":" + str(source["signer"]["run_attempt"]) + ":" + source["source_sha"]
        item = client.api("GET", "/releases/tags/" + source["tag"], allowed=(200, 404))
        if item is None:
            body = canonical({"tag_name": source["tag"], "target_commitish": source["source_sha"], "name": source["tag"], "body": marker, "draft": True, "prerelease": True})
            item = client.api("POST", "/releases", body, allowed=(201,))
        receipt = dict(source, release_id=positive(item.get("id")), payload={"target": "x86_64-unknown-linux-gnu", "name": PAYLOAD, "size": len(payload), "sha256": digest(payload)})
        existing = release(client, receipt)
        existing_assets(existing, {RECEIPT: canonical(receipt), PAYLOAD: payload}, True)
        destination = Path(os.environ["RUNNER_TEMP"]) / RECEIPT
        with destination.open("xb") as output:
            output.write(canonical(receipt))
        return
    require(mode == "publish")
    receipt_bytes = read_file("source-preview-receipt", RECEIPT, 65536)
    receipt = parse(receipt_bytes)
    require(canonical(receipt) == receipt_bytes and set(receipt) == set(source) | {"release_id", "payload"})
    require(all(receipt[key] == value for key, value in source.items()))
    require(receipt["payload"] == {"target": "x86_64-unknown-linux-gnu", "name": PAYLOAD, "size": len(payload), "sha256": digest(payload)})
    bundle = read_file("source-preview-bundle", BUNDLE, MAX_JSON)
    import source_preview_stage_verify as verifier
    preview = {"source_sha": source["source_sha"], "signer_workflow_path": WORKFLOW, "signer_run_id": source["signer"]["run_id"], "signer_run_attempt": source["signer"]["run_attempt"], "receipt_sha256": digest(receipt_bytes), "payload_name": PAYLOAD, "payload_sha256": digest(payload)}
    verifier.verify_attestation(os.environ["PREVIEW_GH"], receipt_bytes, bundle, preview, os.environ["RUNNER_TEMP"])
    expected = {RECEIPT: receipt_bytes, BUNDLE: bundle, PAYLOAD: payload}
    state = client.api("GET", "/releases/" + str(positive(receipt["release_id"])))
    if state.get("draft") is False:
        published = release(client, receipt, False)
        require(published.get("immutable") is True and existing_assets(published, expected) == set(expected))
        return
    present = existing_assets(release(client, receipt), expected)
    for name, data in expected.items():
        if name in present:
            continue
        item = client.request("POST", "uploads.github.com", "/repos/" + REPOSITORY + "/releases/" + str(receipt["release_id"]) + "/assets?name=" + name, data, "application/octet-stream", (201,))
        require(item.get("name") == name and item.get("size") == len(data) and item.get("digest") == "sha256:" + digest(data) and item.get("state") == "uploaded")
    item = release(client, receipt)
    require(existing_assets(item, expected) == set(expected))
    verify_tag(client, source["source_sha"])
    client.api("PATCH", "/releases/" + str(receipt["release_id"]), canonical({"draft": False}), allowed=(200,))
    require(release(client, receipt, False).get("immutable") is True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, TimeoutError, http.client.HTTPException):
        print("source preview publication failed", file=sys.stderr)
        sys.exit(1)
