#!/usr/bin/env python3
"""Admit one exact same-run Actions artifact into a fresh qualification job."""

import argparse
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import tempfile
import zipfile

from source_qualification_execution import API_EVIDENCE_FILES, PREFIX, admit_execution

from owned_tool_source import (HOSTS, canonical, check_hash, descriptor, digest,
                               recipe, recipe_sha, strict_json, validate_receipt)


REPO = "repos/tailrocks/velnor-new/"
MAX_ZIP = 256 * 1024 * 1024
MAX_CONTENT = 512 * 1024 * 1024
MAX_API = 8 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def identity(name):
    workflow = {"commit": os.environ.get("GITHUB_SHA", ""),
                "run_id": os.environ.get("GITHUB_RUN_ID", ""),
                "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", "")}
    require(re.fullmatch(r"[a-f0-9]{40}", workflow["commit"]) and workflow["commit"] != "0" * 40,
            "exact workflow commit required")
    require(all(re.fullmatch(r"[1-9][0-9]*", workflow[key]) for key in ("run_id", "run_attempt")),
            "exact hosted run identity required")
    prefix = "owned-candidate-" + workflow["run_id"] + "-" + workflow["run_attempt"] + "-"
    choices = {prefix + tool + "-" + target: (tool, target)
               for tool in ("mise", "mbx") for target in HOSTS}
    require(name in choices, "artifact name must identify exact current run/attempt/native host")
    tool, target = choices[name]
    require(target == os.environ.get("OWNED_TOOL_TARGET"), "artifact target differs from qualification matrix")
    source = descriptor(os.environ["OWNED_TOOL_SOURCE_JSON"], workflow["commit"])
    require(source["tool"] == tool, "artifact tool differs from approved source")
    return workflow, source, target


def environment(root):
    token = os.environ.get("GH_TOKEN", "")
    require(bool(token), "actions-read token required")
    env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "SSL_CERT_FILE", "SSL_CERT_DIR")
           if key in os.environ}
    home, config = root / "home", root / "gh"
    home.mkdir()
    config.mkdir()
    env.update(HOME=str(home), GH_CONFIG_DIR=str(config), GH_HOST="github.com",
               GH_TOKEN=token, GH_PROMPT_DISABLED="1")
    return env


def gh(endpoint, env, limit, paginate=False):
    require(endpoint == PREFIX or re.fullmatch(re.escape(REPO) +
            r"actions/(?:runs/[1-9][0-9]*(?:/artifacts)?|workflows/[1-9][0-9]*|artifacts/[1-9][0-9]*/zip)", endpoint),
            "GitHub request must use exact owned artifact read endpoint")
    require(not paginate or "/runs/" in endpoint and endpoint.endswith("/artifacts"),
            "only artifact listings may paginate")
    argv = ["gh", "api", endpoint]
    if paginate:
        argv.extend(["--paginate", "--slurp"])
    with tempfile.TemporaryFile() as errors:
        with subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=errors) as process:
            require(process.stdout is not None, "GitHub response stream unavailable")
            data = process.stdout.read(limit + 1)
            if len(data) > limit:
                process.kill()
                raise ValueError("GitHub response size bound exceeded")
            code = process.wait()
            if code:
                # Authenticated diagnostics are retained internally, never echoed with tokens.
                errors.seek(0)
                raise subprocess.CalledProcessError(code, argv, data, errors.read(4096))
    return data


def select_artifact(data, name, workflow):
    pages = strict_json(data)
    require(isinstance(pages, list) and 0 < len(pages) <= 128, "invalid paginated artifact listing")
    matches = []
    for page in pages:
        require(isinstance(page, dict) and isinstance(page.get("artifacts"), list), "invalid artifact page")
        for item in page["artifacts"]:
            require(isinstance(item, dict) and isinstance(item.get("name"), str), "invalid artifact record")
            if item["name"] == name:
                matches.append(item)
    require(len(matches) == 1, "exact same-run artifact must exist exactly once")
    item = matches[0]
    require(type(item.get("id")) is int and item["id"] > 0 and item.get("expired") is False,
            "expired or invalid artifact identity")
    require(type(item.get("size_in_bytes")) is int and 0 < item["size_in_bytes"] <= MAX_ZIP,
            "artifact size bound exceeded")
    run = item.get("workflow_run")
    require(isinstance(run, dict) and type(run.get("id")) is int and
            run["id"] == int(workflow["run_id"]) and run.get("head_sha") == workflow["commit"],
            "artifact API workflow identity mismatch")
    sha = item.get("digest")
    require(isinstance(sha, str) and sha.startswith("sha256:"), "artifact API digest missing")
    check_hash(sha[len("sha256:"):])
    return item


def zip_members(data):
    require(len(data) <= MAX_ZIP, "artifact ZIP size bound exceeded")
    contents, names, total = {}, set(), 0
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        require(0 < len(entries) <= 8, "artifact ZIP member count exceeded")
        for entry in entries:
            name = entry.filename
            kind = stat.S_IFMT(entry.external_attr >> 16)
            require(re.fullmatch(r"[A-Za-z0-9._-]+", name) and name not in (".", "..") and
                    entry.orig_filename == name and canonical(name) not in names,
                    "duplicate or unsafe artifact ZIP member")
            require(not entry.is_dir() and not entry.external_attr & 0x10 and
                    kind in (0, stat.S_IFREG) and not entry.flag_bits & 1 and
                    entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                    "artifact ZIP member must be an unencrypted regular file")
            total += entry.file_size
            require(0 <= entry.file_size <= MAX_CONTENT and total <= MAX_CONTENT,
                    "artifact ZIP expansion bound exceeded")
            names.add(canonical(name))
            with archive.open(entry) as member:
                payload = member.read(entry.file_size + 1)
            require(len(payload) == entry.file_size, "artifact ZIP size mismatch")
            contents[name] = payload
    return contents


def archive_validator():
    path = Path(__file__).with_name("publish-owned-tool-artifacts.py")
    spec = importlib.util.spec_from_file_location("candidate_archive_validator", path)
    require(spec is not None and spec.loader is not None, "archive validator unavailable")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    return helper.validate_archive


def admit_contents(contents, source, target, workflow):
    expected_name = f'{source["tool"]}-{source["version"]}-{target}.tar.gz'
    require(set(contents) == {expected_name, "candidate-receipt.json", "source-receipt.json"},
            "artifact ZIP contains unexpected candidate files")
    receipt = strict_json(contents["candidate-receipt.json"])
    require(isinstance(receipt, dict) and set(receipt) == set(
        "schema status tool version target source workflow artifact version_banner compiler runner recipe behavioral_qualification".split()),
        "unexpected candidate receipt fields")
    require(type(receipt["schema"]) is int and receipt["schema"] == 1 and
            receipt["status"] == "SOURCE_BUILD_CANDIDATE" and receipt["behavioral_qualification"] is None,
            "candidate cannot claim behavioral qualification")
    require(receipt["tool"] == source["tool"] and receipt["version"] == source["version"] and
            receipt["target"] == target and receipt["workflow"] ==
            {**workflow, "recipe_sha256": recipe_sha(source["tool"])} and
            receipt["recipe"] == recipe(source["tool"]), "candidate workflow, recipe, tool or target mismatch")
    source_record = {"commit": source["source_commit"], "tree": source["source_tree"],
                     "archive_sha256": source["archive_sha256"], "receipt_sha256": source["receipt_sha256"],
                     "lockfile_sha256": source["lockfile_sha256"], "base_patch_sha256": source["patch_sha256"],
                     "license_files": source["license_files"]}
    require(receipt["source"] == source_record, "candidate approved source mismatch")
    raw = contents["source-receipt.json"]
    require(digest(raw) == source["receipt_sha256"], "candidate source receipt digest mismatch")
    validate_receipt(raw, source)
    artifact = receipt["artifact"]
    require(isinstance(artifact, dict) and set(artifact) == {"name", "archive_sha256", "binary_sha256"} and
            artifact["name"] == expected_name, "candidate binary container identity mismatch")
    check_hash(artifact["archive_sha256"])
    check_hash(artifact["binary_sha256"])
    archive_validator()(contents[expected_name], {**artifact, "target": target}, source["tool"], source_record)
    return receipt


def download(name, output):
    workflow, source, target = identity(name)
    require(not output.exists() and not output.is_symlink(), "artifact destination already exists")
    workspace = os.environ.get("GITHUB_WORKSPACE")
    require(not workspace or not output.resolve().is_relative_to(Path(workspace).resolve()),
            "artifact destination must be outside trusted workflow checkout")
    with tempfile.TemporaryDirectory(prefix="owned-artifact-read-") as temporary:
        env = environment(Path(temporary))
        api_documents = {}
        execution = admit_execution(lambda endpoint: gh(endpoint, env, MAX_API), api_documents=api_documents)
        listing = gh(REPO + "actions/runs/" + workflow["run_id"] + "/artifacts", env, MAX_API, paginate=True)
        item = select_artifact(listing, name, workflow)
        data = gh(REPO + "actions/artifacts/" + str(item["id"]) + "/zip", env, MAX_ZIP)
    require(digest(data) == item["digest"][len("sha256:"):], "artifact API ZIP digest mismatch")
    contents = zip_members(data)
    receipt = admit_contents(contents, source, target, workflow)
    admission = {"schema": 1, "status": "SAME_RUN_ARTIFACT_ADMITTED", "artifact_id": item["id"],
                 "artifact_name": name, "api_digest": item["digest"], "workflow": workflow,
                 "execution": execution,
                 "tool": source["tool"], "target": target,
                 "candidate_receipt_sha256": digest(contents["candidate-receipt.json"]),
                 "binary_container": receipt["artifact"], "behavioral_qualification": None}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir(mode=0o700)
    for filename, payload in contents.items():
        with (output / filename).open("xb") as file:
            file.write(payload)
    for key, payload in api_documents.items():
        with (output / API_EVIDENCE_FILES[key]).open("xb") as file:
            file.write(payload)
    with (output / "artifact-admission.json").open("x", encoding="utf-8") as file:
        file.write(json.dumps(admission, sort_keys=True, indent=2) + "\n")
    return admission


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        record = download(args.name, args.output.absolute())
    except (ValueError, OSError, KeyError, TypeError, zipfile.BadZipFile, tarfile.TarError,
            subprocess.CalledProcessError) as error:
        raise SystemExit("candidate artifact admission failed: " + str(error)) from error
    print(json.dumps(record, sort_keys=True))


if __name__ == "__main__":
    main()
