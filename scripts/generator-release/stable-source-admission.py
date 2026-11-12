#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import re
import sys
import urllib.request


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def admit():
    value = os.environ
    source = value["GITHUB_SHA"]
    if not re.fullmatch(r"[0-9a-f]{40}", source) or value["GITHUB_WORKFLOW_SHA"] != source:
        raise ValueError("source identity")
    if value["SOURCES_ATTEMPT"] != value["GITHUB_RUN_ATTEMPT"]:
        raise ValueError("producer attempt")
    artifact_id = value["SOURCES_ID"]
    if not re.fullmatch(r"[1-9][0-9]*", artifact_id):
        raise ValueError("artifact ID")
    for field in ("SOURCES_TREE", "SOURCES_ROOT_LOCK", "SOURCES_RUNNER_LOCK"):
        if not re.fullmatch(r"[0-9a-f]{64}", value[field]):
            raise ValueError("source digest")
    if sha("Cargo.lock") != value["SOURCES_ROOT_LOCK"] or sha("crates/velnor-runner/Cargo.lock") != value["SOURCES_RUNNER_LOCK"]:
        raise ValueError("source locks")
    digest = value["SOURCES_DIGEST"]
    digest = digest if digest.startswith("sha256:") else "sha256:" + digest
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        raise ValueError("artifact digest")
    if value["GITHUB_REPOSITORY"] != "tailrocks/velnor-new":
        raise ValueError("repository")
    request = urllib.request.Request(
        "https://api.github.com/repos/tailrocks/velnor-new/actions/artifacts/" + artifact_id,
        headers={"Authorization": "Bearer " + value["GH_TOKEN"], "Accept": "application/vnd.github+json", "User-Agent": "velnor-stable-source-admission"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        raw = response.read(1048577)
    if len(raw) > 1048576:
        raise ValueError("artifact metadata limit")
    item = json.loads(raw)
    if type(item) is not dict or type(item.get("workflow_run")) is not dict:
        raise ValueError("artifact response")
    run = item["workflow_run"]
    if (item.get("id") != int(artifact_id) or item.get("name") != value["SOURCES_NAME"] + "-sources-" + value["GITHUB_RUN_ATTEMPT"]
            or item.get("expired") is not False or item.get("digest") != digest
            or type(item.get("size_in_bytes")) is not int or not 0 < item["size_in_bytes"] <= 536870912
            or run.get("id") != int(value["GITHUB_RUN_ID"]) or run.get("head_sha") != source
            or run.get("repository_id") != int(value["GITHUB_REPOSITORY_ID"])):
        raise ValueError("artifact identity")


try:
    admit()
except (KeyError, OSError, UnicodeError, ValueError) as error:
    print(f"::warning::source artifact unavailable: {error}")
    sys.exit(0)
with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
    output.write("admitted=true\n")
