"""Bind authenticated publication claims to exact measured native evidence."""

import hashlib
import json
import re

from owned_tool_source import BASES, check_hash, recipe, strict_json
from owned_tool_behavior import MISE_CASES, native_host_target, valid_mise_cases
from source_qualification_execution import API_EVIDENCE_FILES, validate_execution_evidence, validate_execution_receipt

PREDICATE = "https://tailrocks.dev/owned-tool-qualification/v1"
CLAIM_FIELDS = "predicate_type passed abi cases report_sha256 qualified_receipt_sha256 candidate_receipt_sha256 sourceartifact_id sourceartifact_api_digest sourceartifact_execution_sha256"
RECEIPT_FIELDS = "schema status tool version target source workflow artifact version_banner compiler runner recipe behavioral_qualification"
BEHAVIOR_FIELDS = "schema passed abi target candidate_receipt_sha256 artifact_admission artifact_admission_sha256 report_sha256 cases report limitation execution_evidence"
ADMISSION_FIELDS = "schema status artifact_id artifact_name api_digest workflow tool target candidate_receipt_sha256 binary_container behavioral_qualification execution"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def closed(value, fields):
    require(isinstance(value, dict) and set(value) == set(fields.split()),
            "unexpected or missing qualification evidence fields")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def exact(left, right):
    return json.dumps(left, sort_keys=True) == json.dumps(right, sort_keys=True)


def expected_cases(tool):
    return MISE_CASES if tool == "mise" else ()


def receipt_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def validate_claim(claim, tool):
    closed(claim, CLAIM_FIELDS)
    abi = recipe(tool)["behavior_abi"]
    cases = expected_cases(tool)
    require(isinstance(abi, str) and abi and cases and claim["abi"] == abi,
            "native behavioral ABI unavailable or mismatched")
    require(claim["predicate_type"] == PREDICATE and claim["passed"] is True,
            "missing passed qualification claim")
    require(type(claim["cases"]) is int and claim["cases"] == len(cases) and claim["cases"] > 0,
            "exact positive native cases required")
    for field in ("report_sha256", "qualified_receipt_sha256", "candidate_receipt_sha256", "sourceartifact_execution_sha256"):
        check_hash(claim[field])
    require(type(claim["sourceartifact_id"]) is int and claim["sourceartifact_id"] > 0,
            "positive source artifact identity required")
    api_digest = claim["sourceartifact_api_digest"]
    require(isinstance(api_digest, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", api_digest),
            "exact source artifact API digest required")
    check_hash(api_digest[7:])


def validate_report(report, receipt, claim):
    closed(report, "root host source_commit source_diff_sha256 upstream_commit binary_sha256 version version_is_distinct results")
    require(report["source_commit"] == receipt["source"]["commit"] and
            report["source_diff_sha256"] == receipt["source"]["base_patch_sha256"] and
            report["upstream_commit"] == BASES[receipt["tool"]][1] and
            report["binary_sha256"] == receipt["artifact"]["binary_sha256"] and
            report["version"] == receipt["version_banner"].strip() and
            report["version_is_distinct"] is True, "native report identity mismatch")
    require(native_host_target(report["host"]) == receipt["target"], "native report host mismatch")
    results = report["results"]
    require(isinstance(results, list) and len(results) == claim["cases"] and
            all(isinstance(case, dict) and case.get("passed") is True and
                isinstance(case.get("case"), str) for case in results), "native cases failed")
    names = [case["case"] for case in results]
    require(len(set(names)) == len(names) and valid_mise_cases(results),
            "native case set differs from fixed qualification recipe")


def validate_admission(behavior, receipt, claim, environment=None):
    admission = behavior["artifact_admission"]
    closed(admission, ADMISSION_FIELDS)
    execution = admission["execution"]
    validate_execution_receipt(execution, environment)
    require(digest(receipt_bytes(execution)) == claim["sourceartifact_execution_sha256"],
            "signed source artifact execution receipt digest mismatch")
    for field in ("commit", "run_id", "run_attempt"):
        require(execution[field] == receipt["workflow"][field], "candidate execution identity mismatch")
    require(exact(behavior["execution_evidence"], {"directory": "execution-" + receipt["target"],
            "api_sha256": execution["api_sha256"]}), "qualified execution evidence locator/digests mismatch")
    expected = {"schema": 1, "status": "SAME_RUN_ARTIFACT_ADMITTED",
        "artifact_id": claim["sourceartifact_id"],
        "artifact_name": "owned-candidate-" + receipt["workflow"]["run_id"] + "-" +
                         receipt["workflow"]["run_attempt"] + "-" + receipt["tool"] + "-" + receipt["target"],
        "api_digest": claim["sourceartifact_api_digest"], "workflow":
        {key: value for key, value in receipt["workflow"].items() if key != "recipe_sha256"},
        "tool": receipt["tool"], "target": receipt["target"],
        "candidate_receipt_sha256": claim["candidate_receipt_sha256"],
        "binary_container": receipt["artifact"], "behavioral_qualification": None, "execution": execution}
    require(exact(admission, expected), "source artifact admission does not match signed claim")
    check_hash(behavior["artifact_admission_sha256"])
    require(digest(receipt_bytes(admission)) == behavior["artifact_admission_sha256"],
            "canonical artifact admission digest mismatch")


def validate_qualified(receipt, report, manifest, artifact, environment=None):
    claim = artifact["qualification"]
    closed(receipt, RECEIPT_FIELDS)
    expected_workflow = {"commit": manifest["workflow_commit"], **manifest["workflow"]}
    expected_artifact = {key: artifact[key] for key in ("name", "archive_sha256", "binary_sha256")}
    require(type(receipt["schema"]) is int and receipt["schema"] == 1 and
            receipt["status"] == "SOURCE_BUILD_CANDIDATE" and
            receipt["tool"] == manifest["tool"] and receipt["version"] == manifest["version"] and
            receipt["target"] == artifact["target"] and exact(receipt["source"], manifest["source"]) and
            exact(receipt["workflow"], expected_workflow) and exact(receipt["artifact"], expected_artifact) and
            exact(receipt["recipe"], recipe(manifest["tool"])), "qualified receipt identity mismatch")
    closed(receipt["compiler"], "rustc_vv linker")
    compiler = dict(line.split(": ", 1) for line in receipt["compiler"]["rustc_vv"].splitlines() if ": " in line)
    require(compiler.get("host") == artifact["target"] and compiler.get("release") == "1.98.1"
            and isinstance(receipt["compiler"]["linker"], str) and receipt["compiler"]["linker"],
            "qualified compiler identity mismatch")
    closed(receipt["runner"], "image_os image_version")
    require(all(isinstance(value, str) and value for value in receipt["runner"].values()),
            "qualified hosted runner evidence missing")
    versions = re.findall(r"(?<![A-Za-z0-9.-])[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?",
                          receipt["version_banner"])
    require(versions == [manifest["version"]] and "DEBUG" not in receipt["version_banner"],
            "qualified native version banner mismatch")
    behavior = receipt["behavioral_qualification"]
    closed(behavior, BEHAVIOR_FIELDS)
    for field in ("passed", "abi", "cases", "candidate_receipt_sha256", "report_sha256"):
        require(exact(behavior[field], claim[field]), "measured behavioral evidence mismatch")
    require(type(behavior["schema"]) is int and behavior["schema"] == 1 and
            behavior["target"] == artifact["target"] and exact(behavior["report"], report) and
            isinstance(behavior["limitation"], str), "qualified behavioral report mismatch")
    candidate = dict(receipt, behavioral_qualification=None)
    require(digest(receipt_bytes(candidate)) == claim["candidate_receipt_sha256"],
            "canonical measured candidate receipt digest mismatch")
    validate_admission(behavior, receipt, claim, environment)
    validate_report(report, receipt, claim)


def stage_qualified_evidence(snapshot, manifest, directory, read_regular, environment=None):
    for artifact in manifest["artifacts"]:
        target, claim = artifact["target"], artifact["qualification"]
        receipt_name, report_name = "qualified-receipt-" + target + ".json", "native-report-" + target + ".json"
        receipt_data = read_regular(directory / receipt_name)
        report_data = read_regular(directory / report_name)
        require(digest(receipt_data) == claim["qualified_receipt_sha256"] and
                digest(report_data) == claim["report_sha256"], "qualified evidence digest mismatch")
        receipt = strict_json(receipt_data)
        validate_qualified(receipt, strict_json(report_data), manifest, artifact, environment)
        behavior = receipt["behavioral_qualification"]
        evidence_directory = "execution-" + target
        documents = {key: read_regular(directory / evidence_directory / name)
                     for key, name in API_EVIDENCE_FILES.items()}
        validate_execution_evidence(behavior["artifact_admission"]["execution"], documents, environment)
        for key, name in API_EVIDENCE_FILES.items():
            (snapshot / (evidence_directory + "-" + name)).write_bytes(documents[key])
        (snapshot / receipt_name).write_bytes(receipt_data)
        (snapshot / report_name).write_bytes(report_data)
