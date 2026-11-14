# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import http.client
import io
import json
import os
from pathlib import Path
import re
import signal
import ssl
import stat
import sys
import time
from urllib.parse import urlsplit
import zipfile

REPOSITORY = "tailrocks/velnor-new"
WORKFLOW = ".github/workflows/product-release-generator.yml"
TARGETS = {"linux": ("x86_64-unknown-linux-gnu", "velnor-actions-x86_64-unknown-linux-gnu"),
           "darwin": ("aarch64-apple-darwin", "velnor-actions-aarch64-apple-darwin")}
PAYLOAD_NAMES = {name for _, name in TARGETS.values()}
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


def read_pair(directory):
    root = Path(directory)
    require(not root.is_symlink() and root.is_dir() and {entry.name for entry in root.iterdir()} == {RECEIPT, BUNDLE})
    result = []
    for name, limit in ((RECEIPT, 65536), (BUNDLE, MAX_JSON)):
        descriptor = os.open(root / name, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            info = os.fstat(descriptor)
            require(stat.S_ISREG(info.st_mode) and 0 < info.st_size <= limit)
            with os.fdopen(descriptor, "rb", closefd=False) as file:
                data = file.read(limit + 1)
            after = os.fstat(descriptor)
            require(len(data) == info.st_size and (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns))
            result.append(data)
        finally:
            os.close(descriptor)
    return result


def parse(data):
    return json.loads(data, object_pairs_hook=unique)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def positive(value):
    require(type(value) is int and 0 < value <= 9007199254740991)
    return value


def candidate_marker(source, candidate):
    require(set(candidate) == {"source_artifact_id", "linux_payload_artifact_id", "darwin_payload_artifact_id"})
    for value in candidate.values():
        positive(value)
    marker = {"schema": 3, "source_sha": source["source_sha"],
              "run_id": positive(source["source"]["run_id"]),
              "targets": source["targets"], "candidate": candidate}
    return "velnor-source-preview:" + canonical(marker).decode()


def payload_identity(key, data):
    target, name = TARGETS[key]
    require(len(data) >= 20)
    if key == "linux":
        require(data[:7] == b"\x7fELF\x02\x01\x01" and data[18:20] == b"\x3e\x00")
    else:
        require(len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe"
                and data[4:8] == b"\x0c\x00\x00\x01"
                and data[12:16] == b"\x02\x00\x00\x00")
    return {"target": target, "name": name, "size": len(data), "sha256": digest(data)}


def receipt_targets(source, candidate, payloads):
    require(set(payloads) == set(TARGETS))
    return {key: {"producer": source["targets"][key]["producer"],
                  "qualification_job_id": source["targets"][key]["qualification_job_id"],
                  "candidate_payload_artifact_id": candidate[key + "_payload_artifact_id"],
                  "payload": payload_identity(key, payloads[key])} for key in TARGETS}


def finalize_receipt(decision, attempt):
    require(set(decision) == {"source", "candidate", "release_id", "targets", "draft"})
    source = decision["source"]
    draft = decision["draft"]
    signer = dict(draft, run_attempt=positive(attempt))
    require(source["source"]["run_id"] == draft["run_id"])
    require(all(source["source"][key] == draft[key] for key in ("workflow_path", "workflow_ref", "workflow_sha")))
    require(source["source"]["run_attempt"] <= draft["run_attempt"] <= signer["run_attempt"])
    candidate_marker(source, decision["candidate"])
    common = {key: value for key, value in source.items() if key != "targets"}
    return canonical(dict(common, candidate_source_artifact_id=decision["candidate"]["source_artifact_id"],
                          release_id=positive(decision["release_id"]), targets=decision["targets"], draft=draft, signer=signer))


def select_pair(item, pairs, payloads):
    assets = item.get("assets")
    require(isinstance(assets, list) and len(assets) <= 4)
    names = [asset.get("name") for asset in assets]
    require(len(names) == len(set(names)) and all(name in {RECEIPT, BUNDLE} | PAYLOAD_NAMES for name in names))
    require(all(asset.get("state") == "uploaded" for asset in assets))
    compatible = []
    for pair in pairs:
        expected = {RECEIPT: pair["receipt"], BUNDLE: pair["bundle"], **{TARGETS[key][1]: data for key, data in payloads.items()}}
        if all(asset.get("size") == len(expected[asset["name"]])
               and asset.get("digest") == "sha256:" + digest(expected[asset["name"]]) for asset in assets):
            compatible.append(pair)
    if compatible:
        first = min(pair["signer_attempt"] for pair in compatible)
        matches = [pair for pair in compatible if pair["signer_attempt"] == first]
        require(len(matches) == 1)
        if item.get("draft") is False:
            require(item.get("immutable") is True and set(names) == {RECEIPT, BUNDLE} | PAYLOAD_NAMES)
        else:
            require(item.get("draft") is True)
        return matches[0]
    require(item.get("draft") is True and not assets and not pairs)
    return None


def artifact_files(client, artifact, expected):
    identity = positive(artifact.get("id"))
    size = positive(artifact.get("size_in_bytes"))
    limit = 257 * 1024 * 1024
    require(size <= limit and isinstance(artifact.get("digest"), str)
            and re.fullmatch("sha256:[0-9a-f]{64}", artifact["digest"]))
    host = "api.github.com"
    path = "/repos/" + REPOSITORY + "/actions/artifacts/" + str(identity) + "/zip"
    authorized = True
    for _ in range(4):
        remaining = client.deadline - time.monotonic()
        require(remaining > 0)
        connection = http.client.HTTPSConnection(host, timeout=min(30, remaining), context=ssl.create_default_context())
        try:
            headers = {"Accept": "application/vnd.github+json", "User-Agent": "velnor-source-preview"}
            if authorized:
                headers["Authorization"] = "Bearer " + client.token
            connection.request("GET", path, headers=headers)
            response = connection.getresponse()
            if response.status in (301, 302, 303, 307, 308):
                redirect = urlsplit(response.getheader("Location", ""))
                require(redirect.scheme == "https" and redirect.hostname is not None
                        and re.fullmatch(r"productionresultssa[0-9]+\.blob\.core\.windows\.net", redirect.hostname)
                        and redirect.username is None and redirect.password is None
                        and redirect.port in (None, 443) and not redirect.fragment)
                host = redirect.hostname
                path = redirect.path + ("?" + redirect.query if redirect.query else "")
                authorized = False
                continue
            require(response.status == 200 and not authorized)
            body = bytearray()
            while len(body) <= limit:
                require(time.monotonic() < client.deadline)
                if connection.sock is not None:
                    connection.sock.settimeout(min(30, client.deadline - time.monotonic()))
                chunk = response.read(min(65536, limit + 1 - len(body)))
                if not chunk:
                    break
                body.extend(chunk)
            require(len(body) == size and "sha256:" + digest(body) == artifact["digest"])
            break
        finally:
            connection.close()
    else:
        raise ValueError("preview artifact redirect limit")
    with zipfile.ZipFile(io.BytesIO(body)) as archive:
        entries = archive.infolist()
        require(len(entries) == len(expected) and {entry.filename for entry in entries} == set(expected))
        result = {}
        for entry in entries:
            require(not entry.flag_bits & 1 and stat.S_ISREG(entry.external_attr >> 16)
                    and entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)
                    and 0 < entry.file_size <= expected[entry.filename])
            data = archive.read(entry)
            require(len(data) == entry.file_size)
            result[entry.filename] = data
        return result


def inventory(client, path, key, maximum_pages):
    result = []
    total = None
    for page in range(1, maximum_pages + 1):
        response = client.api("GET", path + "?per_page=100&page=" + str(page))
        count = response.get("total_count")
        values = response.get(key)
        require(type(count) is int and 0 <= count <= maximum_pages * 100 and isinstance(values, list))
        if total is None:
            total = count
        require(count == total and len(values) <= 100)
        result.extend(values)
        require(len(result) <= total)
        if len(result) == total:
            break
        require(len(values) == 100)
    require(len(result) == total)
    ids = [positive(value.get("id")) for value in result]
    require(len(ids) == len(set(ids)))
    return result


def successful_job(client, source, run_id, attempt, name, allow_failed=False):
    require(positive(attempt) <= 51)
    path = "/actions/runs/" + str(positive(run_id)) + "/attempts/" + str(attempt)
    run = client.api("GET", path)
    require(run.get("id") == run_id and run.get("run_attempt") == attempt
            and run.get("head_sha") == source and run.get("head_branch") == "sdk-preview-" + source
            and run.get("event") == "push" and run.get("path", "").split("@", 1)[0] == WORKFLOW
            and run.get("head_repository", {}).get("full_name") == REPOSITORY)
    jobs = inventory(client, path + "/jobs", "jobs", 10)
    require(all(job.get("run_id") == run_id and job.get("head_sha") == source for job in jobs))
    matches = [job for job in jobs if job.get("name") == name]
    require(len(matches) == 1 and matches[0].get("status") == "completed")
    if allow_failed and matches[0].get("conclusion") != "success":
        return None
    require(matches[0].get("conclusion") == "success")
    return matches[0]["id"]


def find_release(client, tag):
    releases = []
    for page in range(1, 21):
        values = client.api("GET", "/releases?per_page=100&page=" + str(page))
        require(isinstance(values, list) and len(values) <= 100)
        releases.extend(values)
        if len(values) < 100:
            break
    else:
        raise ValueError("preview release inventory limit")
    ids = [positive(item.get("id")) for item in releases]
    require(len(ids) == len(set(ids)))
    matches = [item for item in releases if item.get("tag_name") == tag]
    require(len(matches) <= 1)
    return matches[0] if matches else None


def load_candidate(client, current, marker):
    require(set(marker) == {"schema", "source_sha", "run_id", "targets", "candidate"}
            and type(marker["schema"]) is int and marker["schema"] == 3
            and marker["source_sha"] == current["source_sha"]
            and marker["run_id"] == current["source"]["run_id"] and marker["targets"] == current["targets"])
    candidate = marker["candidate"]
    candidate_marker(current, candidate)
    items = {}
    for key, identity in candidate.items():
        artifact = client.api("GET", "/actions/artifacts/" + str(positive(identity)))
        require(artifact.get("id") == identity and artifact.get("expired") is False
                and artifact.get("workflow_run", {}).get("id") == marker["run_id"]
                and artifact.get("workflow_run", {}).get("head_sha") == current["source_sha"])
        items[key] = artifact
    raw = artifact_files(client, items["source_artifact_id"], {"source-preview-metadata.json": 65536})["source-preview-metadata.json"]
    original = metadata(raw)
    attempt = original["source"]["run_attempt"]
    require(items["source_artifact_id"].get("name") == "source-preview-source-" + str(attempt)
            and original["targets"] == marker["targets"] and original["version"] == current["version"])
    successful_job(client, current["source_sha"], marker["run_id"], attempt, "Source preview publication / Admit source preview")
    payloads = {}
    for key, (_, name) in TARGETS.items():
        item = items[key + "_payload_artifact_id"]
        require(item.get("name") == "source-preview-qualified-payload-" + key + "-" + str(attempt))
        payloads[key] = artifact_files(client, item, {name: MAX_PAYLOAD})[name]
        payload_identity(key, payloads[key])
    return original, candidate, payloads


def verify_pair(client, source, candidate, release_id, payloads, receipt_bytes, bundle):
    receipt = parse(receipt_bytes)
    require(canonical(receipt) == receipt_bytes and set(receipt) == (set(source) - {"targets"}) | {"release_id", "candidate_source_artifact_id", "targets", "draft", "signer"})
    require(all(receipt[key] == value for key, value in source.items() if key != "targets"))
    require(receipt["candidate_source_artifact_id"] == candidate["source_artifact_id"] and receipt["release_id"] == release_id)
    require(receipt["targets"] == receipt_targets(source, candidate, payloads))
    origin = source["source"]
    for role in ("draft", "signer"):
        run = receipt[role]
        require(set(run) == set(origin) and all(run[key] == origin[key] for key in ("workflow_path", "workflow_ref", "workflow_sha", "run_id")))
        positive(run["run_attempt"])
    require(origin["run_attempt"] <= receipt["draft"]["run_attempt"] <= receipt["signer"]["run_attempt"] <= positive(int(os.environ["GITHUB_RUN_ATTEMPT"])) <= 51)
    for role, name in (("source", "Source preview publication / Admit source preview"), ("draft", "Source preview publication / Draft source preview"), ("signer", "Source preview publication / Attest source preview")):
        successful_job(client, source["source_sha"], origin["run_id"], receipt[role]["run_attempt"], name)
    import source_preview_stage_verify as verifier
    preview = {"source_sha": source["source_sha"], "signer_workflow_path": WORKFLOW,
               "signer_run_id": origin["run_id"], "signer_run_attempt": receipt["signer"]["run_attempt"],
               "receipt_sha256": digest(receipt_bytes), **{key + "_payload_" + field: receipt["targets"][key]["payload"][field] for key in TARGETS for field in ("name", "sha256")}}
    verifier.verify_attestation(os.environ["PREVIEW_GH"], receipt_bytes, bundle, preview, os.environ["RUNNER_TEMP"], preview_authority(), client.deadline)
    return {"receipt": receipt_bytes, "bundle": bundle, "signer_attempt": receipt["signer"]["run_attempt"]}


def prepare_pair(client, source, decision, payloads):
    require(set(decision) == {"source", "candidate", "release_id", "targets", "draft"} and decision["source"] == source)
    candidate = decision["candidate"]
    candidate_marker(source, candidate)
    require(decision["targets"] == receipt_targets(source, candidate, payloads))
    release_id = positive(decision["release_id"])
    item = client.api("GET", "/releases/" + str(release_id))
    require(item.get("id") == release_id and item.get("tag_name") == source["tag"]
            and item.get("body") == candidate_marker(source, candidate) and item.get("prerelease") is True
            and type(item.get("draft")) is bool)
    pairs = []
    if item["draft"] is False:
        receipt_bytes, bundle = released_pair(item, source)
        pair = verify_pair(client, source, candidate, release_id, payloads, receipt_bytes, bundle)
        pair["origin"] = "release"
        pairs.append(pair)
    else:
        run_id = source["source"]["run_id"]
        artifacts = inventory(client, "/actions/runs/" + str(run_id) + "/artifacts", "artifacts", 20)
        signed_names = [artifact.get("name") for artifact in artifacts if re.fullmatch(r"source-preview-signed-[1-9][0-9]*", artifact.get("name", ""))]
        require(len(signed_names) == len(set(signed_names)))
        for artifact in artifacts:
            match = re.fullmatch(r"source-preview-signed-([1-9][0-9]*)", artifact.get("name", ""))
            if match is None:
                continue
            attempt = positive(int(match.group(1)))
            require(attempt <= 51 and attempt <= positive(int(os.environ["GITHUB_RUN_ATTEMPT"])))
            require(artifact.get("workflow_run", {}).get("id") == run_id and artifact.get("workflow_run", {}).get("head_sha") == source["source_sha"])
            if artifact.get("expired") is True:
                continue
            require(artifact.get("expired") is False)
            if successful_job(client, source["source_sha"], run_id, attempt, "Source preview publication / Attest source preview", True) is None:
                continue
            files = artifact_files(client, artifact, {RECEIPT: 65536, BUNDLE: MAX_JSON})
            parsed = parse(files[RECEIPT])
            require(isinstance(parsed, dict) and canonical(parsed) == files[RECEIPT])
            if parsed.get("candidate_source_artifact_id") != candidate["source_artifact_id"] or any(parsed.get("targets", {}).get(key, {}).get("candidate_payload_artifact_id") != candidate[key + "_payload_artifact_id"] for key in TARGETS):
                continue
            pair = verify_pair(client, source, candidate, release_id, payloads, files[RECEIPT], files[BUNDLE])
            require(pair["signer_attempt"] == attempt)
            pair.update({"origin": "artifact", "artifact_id": artifact["id"]})
            pairs.append(pair)
    selected = select_pair(item, pairs, payloads)
    if selected is None:
        successful_job(client, source["source_sha"], source["source"]["run_id"], source["source"]["run_attempt"], "Source preview publication / Admit source preview")
        return {"sign": True}
    return dict(selected, sign=False)


def released_pair(item, source):
    import source_preview_bootstrap as bootstrap
    require(item.get("draft") is False and item.get("immutable") is True)
    entries = item.get("assets")
    require(isinstance(entries, list) and len(entries) == 4
            and {entry.get("name") for entry in entries} == {RECEIPT, BUNDLE} | PAYLOAD_NAMES)
    prefix = "https://github.com/" + REPOSITORY + "/releases/download/" + source["tag"] + "/"
    transport = bootstrap.GitHubTransport("")
    result = []
    for name, limit in ((RECEIPT, 65536), (BUNDLE, MAX_JSON)):
        size, sha = bootstrap.asset_metadata(item, name, limit, prefix + name)
        result.append(bootstrap.checked_asset(transport, prefix + name, size, sha, limit))
    return result


def metadata(data):
    value = parse(data)
    source = os.environ["GITHUB_SHA"]
    tag = "sdk-preview-" + source
    tag_ref = "refs/tags/" + tag
    require(canonical(value) == data and re.fullmatch("[0-9a-f]{40}", source))
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_EVENT_NAME") == "push"
            and os.environ.get("GITHUB_REF") == tag_ref and os.environ.get("GITHUB_WORKFLOW_REF") == REPOSITORY + "/" + WORKFLOW + "@" + tag_ref
            and os.environ.get("GITHUB_WORKFLOW_SHA") == source)
    require(set(value) == {"schema", "repository", "version", "source_sha", "tag", "source", "ci", "qualification", "targets"})
    require(type(value["schema"]) is int and value["schema"] == 3 and value["repository"] == REPOSITORY
            and value["source_sha"] == source and value["tag"] == tag
            and isinstance(value["version"], str) and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value["version"]))
    for key, path in (("source", WORKFLOW), ("ci", ".github/workflows/ci.yml"), ("qualification", ".github/workflows/source-qualification.yml")):
        run = value[key]
        require(set(run) == {"workflow_path", "workflow_ref", "workflow_sha", "run_id", "run_attempt"})
        ref = tag_ref if key == "source" else "refs/heads/main"
        require(run["workflow_path"] == path and run["workflow_ref"] == REPOSITORY + "/" + path + "@" + ref
                and run["workflow_sha"] == source)
        positive(run["run_id"]); positive(run["run_attempt"])
    require(value["source"]["run_id"] == positive(int(os.environ["GITHUB_RUN_ID"]))
            and value["source"]["run_attempt"] <= positive(int(os.environ["GITHUB_RUN_ATTEMPT"])) <= 51)
    require(set(value["targets"]) == set(TARGETS))
    for target in value["targets"].values():
        require(set(target) == {"producer", "qualification_job_id"})
        positive(target["qualification_job_id"])
        producer = target["producer"]
        require(set(producer) == {"producer_run_id", "producer_run_attempt", "payload_artifact_id", "payload_archive_sha256"})
        for key in ("producer_run_id", "producer_run_attempt", "payload_artifact_id"):
            positive(producer[key])
        require(isinstance(producer["payload_archive_sha256"], str)
                and re.fullmatch("[0-9a-f]{64}", producer["payload_archive_sha256"]))
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
    marker = candidate_marker({**receipt, "targets": {key: {"producer": value["producer"], "qualification_job_id": value["qualification_job_id"]} for key, value in receipt["targets"].items()}}, {"source_artifact_id": receipt["candidate_source_artifact_id"], **{key + "_payload_artifact_id": value["candidate_payload_artifact_id"] for key, value in receipt["targets"].items()}})
    require(item.get("id") == receipt["release_id"] and item.get("tag_name") == receipt["tag"] and item.get("body") == marker and item.get("prerelease") is True)
    require(item.get("draft") is require_draft)
    return item


def existing_assets(item, expected, allow_unknown_bundle=False):
    assets = item.get("assets")
    require(isinstance(assets, list) and len(assets) <= 4)
    result = set()
    for asset in assets:
        name = asset.get("name")
        require(name in {RECEIPT, BUNDLE} | PAYLOAD_NAMES and name not in result and asset.get("state") == "uploaded")
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


def publish_verified(client, source, receipt, receipt_bytes, bundle, payloads):
    expected = {RECEIPT: receipt_bytes, BUNDLE: bundle, **{TARGETS[key][1]: data for key, data in payloads.items()}}
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
    published = release(client, receipt, False)
    require(published.get("immutable") is True and existing_assets(published, expected) == set(expected))


def main():
    preview_authority()
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("preview publication deadline")))
    signal.setitimer(signal.ITIMER_REAL, 180)
    source = metadata(read_file("source-preview-metadata", "source-preview-metadata.json", 65536))
    payloads = {key: read_file("source-preview-payload-" + key, name, MAX_PAYLOAD) for key, (_, name) in TARGETS.items()}
    for key, data in payloads.items():
        payload_identity(key, data)
    client = GitHub()
    verify_tag(client, source["source_sha"])
    mode = os.environ["PREVIEW_MODE"]
    if mode == "draft":
        successful_job(client, source["source_sha"], source["source"]["run_id"], source["source"]["run_attempt"], "Source preview publication / Admit source preview")
        candidate = {"source_artifact_id": positive(int(os.environ["SOURCE_ARTIFACT_ID"])), **{key + "_payload_artifact_id": positive(int(os.environ[key.upper() + "_PAYLOAD_ARTIFACT_ID"])) for key in TARGETS}}
        item = find_release(client, source["tag"])
        if item is None:
            marker = candidate_marker(source, candidate)
            body = canonical({"tag_name": source["tag"], "target_commitish": source["source_sha"], "name": source["tag"], "body": marker, "draft": True, "prerelease": True})
            item = client.api("POST", "/releases", body, allowed=(201,))
        else:
            body = item.get("body")
            prefix = "velnor-source-preview:"
            require(isinstance(body, str) and body.startswith(prefix))
            encoded = body[len(prefix):].encode()
            original_marker = parse(encoded)
            require(canonical(original_marker) == encoded)
            source, candidate, payloads = load_candidate(client, source, original_marker)
            marker = candidate_marker(source, candidate)
            require(body == marker)
            verify_tag(client, source["source_sha"])
        decision = {"source": source, "candidate": candidate, "release_id": positive(item.get("id")), "targets": receipt_targets(source, candidate, payloads), "draft": dict(source["source"], run_attempt=positive(int(os.environ["GITHUB_RUN_ATTEMPT"]))) }
        current = client.api("GET", "/releases/" + str(decision["release_id"]))
        require(current.get("id") == decision["release_id"] and current.get("tag_name") == source["tag"] and current.get("body") == marker and current.get("prerelease") is True and type(current.get("draft")) is bool)
        require(current.get("draft") is True or current.get("immutable") is True)
        assets = current.get("assets")
        require(isinstance(assets, list) and len(assets) <= 4)
        names = [asset.get("name") for asset in assets]
        require(len(names) == len(set(names)) and all(name in {RECEIPT, BUNDLE} | PAYLOAD_NAMES for name in names))
        for asset in assets:
            require(asset.get("state") == "uploaded")
            for key, (_, name) in TARGETS.items():
                if asset["name"] == name:
                    require(asset.get("size") == len(payloads[key]) and asset.get("digest") == "sha256:" + digest(payloads[key]))
        destination = Path(os.environ["RUNNER_TEMP"]) / "source-preview-decision.json"
        with destination.open("xb") as output:
            output.write(canonical(decision))
        selected = prepare_pair(client, source, decision, payloads)
        verify_tag(client, source["source_sha"])
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write("".join(key + "=" + str(value) + "\n" for key, value in sorted(candidate.items())))
            output.write("sign=" + str(selected["sign"]).lower() + "\n")
            if not selected["sign"]:
                output.write("pair_origin=" + selected["origin"] + "\n")
                output.write("pair_id=" + str(selected.get("artifact_id", decision["release_id"])) + "\n")
                output.write("receipt_sha256=" + digest(selected["receipt"]) + "\nbundle_sha256=" + digest(selected["bundle"]) + "\n")
        return
    if mode == "prepare":
        decision_bytes = read_file("source-preview-decision", "source-preview-decision.json", 65536)
        decision = parse(decision_bytes)
        require(canonical(decision) == decision_bytes)
        require(set(decision) == {"source", "candidate", "release_id", "targets", "draft"} and decision["source"] == source)
        require(set(decision["draft"]) == set(source["source"])
                and all(decision["draft"][key] == source["source"][key] for key in ("workflow_path", "workflow_ref", "workflow_sha", "run_id"))
                and source["source"]["run_attempt"] <= positive(decision["draft"]["run_attempt"]) <= positive(int(os.environ["GITHUB_RUN_ATTEMPT"])) <= 51)
        candidate = {"source_artifact_id": positive(int(os.environ["SOURCE_ARTIFACT_ID"])), **{key + "_payload_artifact_id": positive(int(os.environ[key.upper() + "_PAYLOAD_ARTIFACT_ID"])) for key in TARGETS}}
        require(decision["candidate"] == candidate and decision["targets"] == receipt_targets(source, candidate, payloads))
        successful_job(client, source["source_sha"], source["source"]["run_id"], source["source"]["run_attempt"], "Source preview publication / Admit source preview")
        successful_job(client, source["source_sha"], source["source"]["run_id"], decision["draft"]["run_attempt"], "Source preview publication / Draft source preview")
        if os.environ["SIGN"] == "true":
            selected = {"sign": True, "receipt": finalize_receipt(decision, positive(int(os.environ["GITHUB_RUN_ATTEMPT"])))}
        else:
            require(os.environ["SIGN"] == "false")
            if os.environ["PAIR_ORIGIN"] == "artifact":
                receipt_bytes, bundle = read_pair("source-preview-signed")
            else:
                require(os.environ["PAIR_ORIGIN"] == "release")
                item = client.api("GET", "/releases/" + str(positive(int(os.environ["PAIR_ID"]))))
                receipt_bytes, bundle = released_pair(item, source)
            require(digest(receipt_bytes) == os.environ["RECEIPT_SHA256"] and digest(bundle) == os.environ["BUNDLE_SHA256"])
            selected = verify_pair(client, source, candidate, decision["release_id"], payloads, receipt_bytes, bundle)
            selected.update({"sign": False, "origin": os.environ["PAIR_ORIGIN"], "artifact_id": positive(int(os.environ["PAIR_ID"]))})
        verify_tag(client, source["source_sha"])
        with (Path(os.environ["RUNNER_TEMP"]) / RECEIPT).open("xb") as output:
            output.write(selected["receipt"])
        if not selected["sign"]:
            with (Path(os.environ["RUNNER_TEMP"]) / BUNDLE).open("xb") as output:
                output.write(selected["bundle"])
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write("sign=" + str(selected["sign"]).lower() + "\n")
            if not selected["sign"]:
                output.write("pair_origin=" + selected["origin"] + "\n")
                output.write("pair_id=" + str(selected.get("artifact_id", decision["release_id"])) + "\n")
                output.write("receipt_sha256=" + digest(selected["receipt"]) + "\nbundle_sha256=" + digest(selected["bundle"]) + "\n")
        return
    require(mode == "publish")
    if os.environ["PAIR_ORIGIN"] == "artifact":
        receipt_bytes, bundle = read_pair("source-preview-signed")
    else:
        require(os.environ["PAIR_ORIGIN"] == "release")
        item = client.api("GET", "/releases/" + str(positive(int(os.environ["PAIR_ID"]))))
        receipt_bytes, bundle = released_pair(item, source)
    receipt = parse(receipt_bytes)
    if os.environ["PAIR_ORIGIN"] == "release":
        require(digest(receipt_bytes) == os.environ["RECEIPT_SHA256"] and digest(bundle) == os.environ["BUNDLE_SHA256"])
    candidate = {"source_artifact_id": positive(int(os.environ["SOURCE_ARTIFACT_ID"])), **{key + "_payload_artifact_id": positive(int(os.environ[key.upper() + "_PAYLOAD_ARTIFACT_ID"])) for key in TARGETS}}
    verify_pair(client, source, candidate, positive(receipt["release_id"]), payloads, receipt_bytes, bundle)
    publish_verified(client, source, receipt, receipt_bytes, bundle, payloads)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, TimeoutError, http.client.HTTPException, zipfile.BadZipFile):
        print("source preview publication failed", file=sys.stderr)
        sys.exit(1)
