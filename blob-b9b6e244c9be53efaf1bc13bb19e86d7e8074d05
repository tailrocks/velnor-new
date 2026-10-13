"""Isolated publication policy tests; never contact GitHub or execute tools."""

import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import owned_tool_qualification_evidence as Q
import immutable_publication as I
import owned_tool_source as S

SPEC = importlib.util.spec_from_file_location("publisher",
    Path(__file__).with_name("publish-owned-tool-artifacts.py"))
P = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(P)


def binary(target):
    data = bytearray(64)
    kind, machine = P.TARGETS[target]
    if kind == "elf":
        data[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<HH", data, 16, 2, machine)
    else:
        data[:4] = b"\xcf\xfa\xed\xfe"
        struct.pack_into("<I", data, 4, machine)
        struct.pack_into("<I", data, 12, 2)
    return bytes(data)


def archive(tool, target, extra=None):
    buffer = io.BytesIO()
    native = binary(target)
    members = [("mise/bin/mise" if tool == "mise" else "mbx", native, 0o755),
               ("mise/LICENSE" if tool == "mise" else "LICENSE", b"license", 0o644)]
    if extra:
        members.append(extra)
    with tarfile.open(fileobj=buffer, mode="w:gz") as output:
        for name, data, mode in members:
            entry = tarfile.TarInfo(name)
            entry.size, entry.mode = len(data), mode
            output.addfile(entry, io.BytesIO(data))
    return buffer.getvalue()


def fixture(tool="mise"):
    manifest = {"schema": 1, "tool": tool, "version": "1.2.3-owned-cargo-wrapper",
        "tag": tool + "-v1.2.3-owned-cargo-wrapper", "workflow_commit": "a" * 40,
        "source": {"commit": "b" * 40, "tree": "c" * 40,
                   "archive_sha256": "d" * 64, "lockfile_sha256": "e" * 64,
                   "base_patch_sha256": "f" * 64, "receipt_sha256": "0" * 64,
                   "license_files": {"LICENSE": P.sha(b"license")}},
        "workflow": {"run_id": "12", "run_attempt": "1", "recipe_sha256": P.recipe_sha(tool)},
        "artifacts": []}
    manifest["source"]["receipt_sha256"] = P.sha(receipt_bytes(manifest))
    files = {}
    for target in P.TARGETS:
        name = f"{tool}-{manifest['version']}-{target}.tar.gz"
        data = archive(tool, target)
        files[name] = data
        artifact = {"target": target, "name": name,
            "archive_sha256": P.sha(data), "binary_sha256": P.sha(binary(target))}
        qualification, evidence = qualified_files(manifest, artifact)
        artifact["qualification"] = qualification
        manifest["artifacts"].append(artifact)
        files.update(evidence)
    manifest["source"]["receipt_sha256"] = P.sha(receipt_bytes(manifest))
    return manifest, files


def receipt_bytes(manifest):
    source = manifest["source"]
    repository, base = S.BASES[manifest["tool"]]
    return json.dumps({"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": manifest["tool"],
        "upstream_repository": "https://github.com/" + repository, "upstream_base_commit": base,
        "source_commit": source["commit"], "source_tree": source["tree"],
        "source_archive": {"name": "source.tar", "sha256": source["archive_sha256"]},
        "lockfile": {"path": "Cargo.lock", "sha256": source["lockfile_sha256"]},
        "base_patch": {"name": "base.patch", "sha256": source["base_patch_sha256"]},
        "license_files": source["license_files"], "required_hosts": S.HOSTS,
        "publication": None, "behavioral_qualification": None,
        "signed_build_provenance": None}).encode()


def attestation(manifest, artifact):
    return [{"verificationResult": {"statement": {
        "_type": "https://in-toto.io/Statement/v1", "predicateType": P.PREDICATE,
        "subject": [{"name": artifact["name"], "digest":
                     {"sha256": artifact["archive_sha256"]}}],
        "predicate": {"schema": 1, "tool": manifest["tool"],
            "version": manifest["version"], "source": manifest["source"],
            "workflow": manifest["workflow"], "target": artifact["target"],
            "binary_sha256": artifact["binary_sha256"], "qualification":
            {key: value for key, value in artifact["qualification"].items() if key != "predicate_type"}}}}}]



def qualified_files(manifest, artifact):
    target = artifact["target"]
    system, machine = {"x86_64-unknown-linux-gnu": ("Linux", "x86_64"),
        "aarch64-unknown-linux-gnu": ("Linux", "aarch64"),
        "aarch64-apple-darwin": ("Darwin", "arm64")}[target]
    report = {"root": "/fixture", "host": {"system": system, "machine": machine},
        "source_commit": manifest["source"]["commit"],
        "source_diff_sha256": manifest["source"]["base_patch_sha256"],
        "upstream_commit": S.BASES[manifest["tool"]][1],
        "binary_sha256": artifact["binary_sha256"], "version": manifest["version"],
        "version_is_distinct": True,
        "results": [{"case": case, "passed": True} for case in Q.MISE_CASES]}
    report_data = Q.receipt_bytes(report)
    receipt = {"schema": 1, "status": "SOURCE_BUILD_CANDIDATE", "tool": manifest["tool"],
        "version": manifest["version"], "target": target, "source": manifest["source"],
        "workflow": {"commit": manifest["workflow_commit"], **manifest["workflow"]},
        "artifact": {key: artifact[key] for key in ("name", "archive_sha256", "binary_sha256")},
        "version_banner": manifest["version"], "compiler": {
            "rustc_vv": f"release: 1.99.0\nhost: {target}", "linker": "fixture linker"},
        "runner": {"image_os": "fixture", "image_version": "1"},
        "recipe": S.recipe(manifest["tool"]), "behavioral_qualification": None}
    candidate_sha = P.sha(Q.receipt_bytes(receipt))
    admission = {"schema": 1, "status": "SAME_RUN_ARTIFACT_ADMITTED", "artifact_id": 101,
        "artifact_name": "owned-candidate-12-1-" + manifest["tool"] + "-" + target,
        "api_digest": "sha256:" + "f" * 64,
        "workflow": {"commit": manifest["workflow_commit"], "run_id": "12", "run_attempt": "1"},
        "tool": manifest["tool"], "target": target, "candidate_receipt_sha256": candidate_sha,
        "binary_container": receipt["artifact"], "behavioral_qualification": None}
    receipt["behavioral_qualification"] = {"schema": 1, "passed": True,
        "abi": S.recipe(manifest["tool"])["behavior_abi"], "target": target,
        "candidate_receipt_sha256": candidate_sha, "artifact_admission": admission,
        "artifact_admission_sha256": P.sha(Q.receipt_bytes(admission)),
        "report_sha256": P.sha(report_data), "cases": len(Q.MISE_CASES),
        "report": report, "limitation": "fixture only"}
    receipt_data = Q.receipt_bytes(receipt)
    claim = {"predicate_type": P.PREDICATE, "passed": True,
        "abi": receipt["behavioral_qualification"]["abi"], "cases": len(Q.MISE_CASES),
        "report_sha256": P.sha(report_data), "qualified_receipt_sha256": P.sha(receipt_data),
        "candidate_receipt_sha256": candidate_sha, "sourceartifact_id": 101,
        "sourceartifact_api_digest": admission["api_digest"]}
    return claim, {"qualified-receipt-" + target + ".json": receipt_data,
                   "native-report-" + target + ".json": report_data}
