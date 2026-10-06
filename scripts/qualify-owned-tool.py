#!/usr/bin/env python3
"""Measure a built native candidate; never rebuild or publish it."""

import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import tempfile

from owned_tool_source import BASES, check_hash, descriptor, strict_json, validate_receipt
from owned_tool_behavior import MISE_ABI, native_host_target, valid_mise_cases
from source_qualification_execution import API_EVIDENCE_FILES, validate_execution_evidence
from owned_mbx_observation import observe as observe_mbx, smoke_matches


def require(condition, message):
    if not condition:
        raise ValueError(message)


def closed(value, fields):
    require(isinstance(value, dict) and set(value) == set(fields.split()),
            "unexpected or missing candidate fields")


def native_target():
    target = native_host_target({"system": platform.system(), "machine": platform.machine()})
    require(target is not None, "unsupported native qualification host")
    return target


def archive_helpers():
    path = Path(__file__).with_name("publish-owned-tool-artifacts.py")
    spec = importlib.util.spec_from_file_location("owned_archive_validation", path)
    require(spec is not None and spec.loader is not None, "archive validator unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build_helpers():
    path = Path(__file__).with_name("build-owned-tool.py")
    spec = importlib.util.spec_from_file_location("owned_build_recipe", path)
    require(spec is not None and spec.loader is not None, "build recipe unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def source_record(source):
    return {"commit": source["source_commit"], "tree": source["source_tree"],
            "archive_sha256": source["archive_sha256"],
            "receipt_sha256": source["receipt_sha256"],
            "lockfile_sha256": source["lockfile_sha256"],
            "base_patch_sha256": source["patch_sha256"],
            "license_files": source["license_files"]}


def admit_artifact(directory, receipt, receipt_digest, helpers):
    raw = helpers.read_regular(directory / "artifact-admission.json")
    admission = strict_json(raw)
    closed(admission, "schema status artifact_id artifact_name api_digest workflow tool target "
           "execution candidate_receipt_sha256 binary_container behavioral_qualification")
    workflow = {key: receipt["workflow"][key] for key in ("commit", "run_id", "run_attempt")}
    name = f"owned-candidate-{workflow['run_id']}-{workflow['run_attempt']}-{receipt['tool']}-{receipt['target']}"
    require(type(admission["schema"]) is int and admission["schema"] == 1 and
            admission["status"] == "SAME_RUN_ARTIFACT_ADMITTED" and
            type(admission["artifact_id"]) is int and admission["artifact_id"] > 0,
            "artifact API identity missing")
    require(admission["artifact_name"] == name and admission["workflow"] == workflow and
            admission["tool"] == receipt["tool"] and admission["target"] == receipt["target"] and
            admission["candidate_receipt_sha256"] == receipt_digest and
            admission["binary_container"] == receipt["artifact"] and
            admission["behavioral_qualification"] is None, "artifact admission binding mismatch")
    require(isinstance(admission["api_digest"], str) and
            admission["api_digest"].startswith("sha256:"), "artifact API ZIP digest missing")
    check_hash(admission["api_digest"][len("sha256:"):])
    api_documents = {key: helpers.read_regular(directory / name)
                     for key, name in API_EVIDENCE_FILES.items()}
    execution = validate_execution_evidence(admission["execution"], api_documents)
    require({key: execution[key] for key in workflow} == workflow,
            "execution API origin differs from candidate workflow identity")
    return admission, hashlib.sha256(raw).hexdigest(), api_documents


def admit(arguments, helpers):
    directory = arguments.candidate_directory.resolve(strict=True)
    raw = helpers.read_regular(directory / "candidate-receipt.json")
    receipt = strict_json(raw)
    closed(receipt, "schema status tool version target source "
           "workflow artifact version_banner compiler runner recipe behavioral_qualification")
    require(type(receipt["schema"]) is int and receipt["schema"] == 1 and
            receipt["status"] == "SOURCE_BUILD_CANDIDATE", "candidate receipt status")
    require(arguments.tool == receipt["tool"] and arguments.target == receipt["target"]
            and arguments.target == native_target(), "tool or native host mismatch")
    builder = build_helpers()
    closed(receipt["workflow"], "commit run_id run_attempt recipe_sha256")
    expected_workflow = {**builder.workflow_identity(),
                         "recipe_sha256": builder.recipe_sha(arguments.tool)}
    require(receipt["workflow"] == expected_workflow and
            receipt["recipe"] == builder.recipe(arguments.tool),
            "candidate workflow or closed build recipe mismatch")
    closed(receipt["compiler"], "rustc_vv linker")
    require(builder.compiler_identity(receipt["compiler"]["rustc_vv"]) == arguments.target
            and isinstance(receipt["compiler"]["linker"], str)
            and bool(receipt["compiler"]["linker"]), "candidate native compiler evidence mismatch")
    closed(receipt["runner"], "image_os image_version")
    require(all(isinstance(value, str) and value for value in receipt["runner"].values()),
            "candidate hosted runner evidence missing")
    source = descriptor(os.environ["OWNED_TOOL_SOURCE_JSON"], receipt["workflow"]["commit"])
    require(source["tool"] == arguments.tool and source["version"] == receipt["version"]
            and source_record(source) == receipt["source"],
            "candidate source identity mismatch")
    source_receipt = helpers.read_regular(directory / "source-receipt.json")
    require(hashlib.sha256(source_receipt).hexdigest() == source["receipt_sha256"],
            "source receipt digest mismatch")
    validate_receipt(source_receipt, source)
    require(receipt["behavioral_qualification"] is None, "candidate already claims qualification")
    closed(receipt["artifact"], "name archive_sha256 binary_sha256")
    name = f"{arguments.tool}-{receipt['version']}-{arguments.target}.tar.gz"
    require(receipt["artifact"]["name"] == name, "unexpected archive name")
    data = helpers.read_regular(directory / name)
    artifact = dict(receipt["artifact"], target=arguments.target)
    helpers.validate_archive(data, artifact, arguments.tool, receipt["source"])
    receipt_digest = hashlib.sha256(raw).hexdigest()
    admission, admission_digest, api_documents = admit_artifact(
        directory, receipt, receipt_digest, helpers)
    return receipt, data, receipt_digest, admission, admission_digest, api_documents


def write_exclusive(path, receipt):
    content = (json.dumps(receipt, sort_keys=True, indent=2) + "\n").encode()
    write_exclusive_bytes(path, content)


def write_exclusive_bytes(path, content):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(content)


def observe_admitted_mbx(arguments, receipt, archive, input_digest, admission,
                         admission_digest, api_documents, execution_directory):
    with tempfile.TemporaryDirectory(prefix="owned-mbx-observation-") as temporary:
        root = Path(temporary).resolve()
        binary = root / "mbx"
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
            stream = bundle.extractfile("mbx")
            require(stream is not None, "admitted MBX binary unavailable")
            binary.write_bytes(stream.read())
        binary.chmod(0o500)
        report = observe_mbx(binary, root, receipt, input_digest)
        report_bytes = (json.dumps(report, sort_keys=True, indent=2) + "\n").encode()
        observation_matches = (smoke_matches(report) and report.get("source") == receipt["source"] and
            report.get("target") == arguments.target and
            native_host_target(report.get("host")) == arguments.target and
            report.get("artifact") == receipt["artifact"] and
            report.get("binary_sha256") == receipt["artifact"]["binary_sha256"] and
            report.get("candidate_receipt_sha256") == input_digest and
            report.get("version") == receipt["version_banner"].strip() and
            report.get("status") == "OBSERVED_MBX_SMOKE_ONLY" and
            report.get("passed") is False and report.get("abi") is None and
            report.get("native_authority") is None)
        envelope = {"schema": 1, "status": ("MBX_NATIVE_QUALIFICATION_UNAVAILABLE"
            if observation_matches else "MBX_OBSERVATION_REJECTED"),
            "tool": "mbx", "target": arguments.target, "candidate": receipt,
            "candidate_receipt_sha256": input_digest,
            "artifact_admission": admission, "artifact_admission_sha256": admission_digest,
            "execution_evidence": {"directory": execution_directory.name,
                                   "api_sha256": admission["execution"]["api_sha256"]},
            "observation_report_sha256": hashlib.sha256(report_bytes).hexdigest(),
            "passed": False, "abi": None, "native_authority": None}
        execution_directory.mkdir(mode=0o700)
        for key, filename in API_EVIDENCE_FILES.items():
            write_exclusive_bytes(execution_directory / filename, api_documents[key])
        write_exclusive_bytes(arguments.report, report_bytes)
        write_exclusive(arguments.receipt, envelope)
        require(observation_matches, "MBX observation rejected; raw evidence preserved")
        raise ValueError("MBX native qualification is unavailable; observations preserved")


def qualify(arguments):
    execution_directory = arguments.receipt.parent / ("execution-" + arguments.target)
    require(arguments.receipt.absolute() != arguments.report.absolute(),
            "receipt and native report destinations must differ")
    require(not any(path.exists() or path.is_symlink()
                    for path in (arguments.receipt, arguments.report, execution_directory)),
            "qualification destination already exists")
    helpers = archive_helpers()
    receipt, archive, input_digest, admission, admission_digest, api_documents = admit(arguments, helpers)
    if arguments.tool == "mbx":
        observe_admitted_mbx(arguments, receipt, archive, input_digest, admission,
                            admission_digest, api_documents, execution_directory)
    with tempfile.TemporaryDirectory(prefix="owned-native-qualification-") as temporary:
        root = Path(temporary).resolve()
        binary = root / "mise"
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
            stream = bundle.extractfile("mise/bin/mise")
            require(stream is not None, "native binary unavailable")
            binary.write_bytes(stream.read())
        binary.chmod(0o500)
        require(hashlib.sha256(binary.read_bytes()).hexdigest() ==
                receipt["artifact"]["binary_sha256"], "extracted binary mismatch")
        report_path = root / "native-report.json"
        command = [sys.executable, str(Path(__file__).with_name("owned_mise_qualification.py")),
                   "--mise", str(binary), "--source-commit", receipt["source"]["commit"],
                   "--upstream-commit", BASES[arguments.tool][1],
                   "--source-diff-sha256", receipt["source"]["base_patch_sha256"],
                   "--expected-version", receipt["version"], "--output", str(report_path)]
        completed = subprocess.run(command, check=False, timeout=600)
        require(report_path.is_file(), "native qualification report missing")
        report_bytes = report_path.read_bytes()
        report = strict_json(report_bytes)
        passed = (completed.returncode == 0 and report["version_is_distinct"] is True and
                report["version"] == receipt["version_banner"].strip() and
                report["binary_sha256"] == receipt["artifact"]["binary_sha256"] and
                report["source_commit"] == receipt["source"]["commit"] and
                report["source_diff_sha256"] == receipt["source"]["base_patch_sha256"] and
                report["upstream_commit"] == BASES[arguments.tool][1] and
                native_host_target(report["host"]) == arguments.target and
                valid_mise_cases(report["results"]))
        receipt["behavioral_qualification"] = {
            "schema": 1, "passed": passed, "abi": MISE_ABI,
            "target": arguments.target, "candidate_receipt_sha256": input_digest,
            "artifact_admission": admission, "artifact_admission_sha256": admission_digest,
            "execution_evidence": {"directory": execution_directory.name,
                                   "api_sha256": admission["execution"]["api_sha256"]},
            "report_sha256": hashlib.sha256(report_bytes).hexdigest(),
            "cases": len(report["results"]), "report": report,
            "limitation": "Digest validation does not seal exec against concurrent path mutation."}
        execution_directory.mkdir(mode=0o700)
        for key, filename in API_EVIDENCE_FILES.items():
            write_exclusive_bytes(execution_directory / filename, api_documents[key])
        write_exclusive_bytes(arguments.report, report_bytes)
        write_exclusive(arguments.receipt, receipt)
        require(passed, "native behavioral qualification failed; failed receipt preserved")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tool", required=True, choices=("mise", "mbx"))
    parser.add_argument("--target", required=True)
    parser.add_argument("--candidate-directory", required=True, type=Path)
    parser.add_argument("--receipt", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        qualify(arguments)
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError,
            subprocess.TimeoutExpired) as error:
        raise SystemExit("qualification failed: " + str(error)) from error


if __name__ == "__main__":
    main()
