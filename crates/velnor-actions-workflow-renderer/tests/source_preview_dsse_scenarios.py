# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import time

root = Path(sys.argv[1])
source_preview = root / "crates/velnor-actions-mise/src/source_preview"
sys.path.insert(0, str(source_preview))
import source_preview_bootstrap as bootstrap
import source_preview_stage_verify as verifier

fixture = root / "crates/velnor-actions-workflow-renderer/tests/fixtures/source-preview-dsse"
context = json.loads((fixture / "context.json").read_bytes())
receipt = (fixture / "SDK_PREVIEW_RECEIPT.json").read_bytes()
bundle = (fixture / "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl").read_bytes()
root_file = fixture / "trusted-root.jsonl"
gh = os.environ["VELNOR_GH_BINARY"]
workflow = context["signer_workflow"]
authority = {
    "repository": context["repository"],
    "path": workflow,
    "ref": f"{context['repository']}/{workflow}@{context['signer_sha']}",
    "sha": context["signer_sha"],
}
policy = {
    "repository": context["repository"],
    "repository_id": context["repository_id"],
    "owner": context["repository"].split("/", 1)[0],
    "owner_id": str(context["owner_id"]),
    "source_ref": context["source_ref"],
    "trigger": "workflow_dispatch",
    "trusted_root": root_file,
}
preview = {
    "source_sha": context["source_sha"],
    "signer_workflow_path": context["caller_workflow"],
    "signer_run_id": context["run_id"],
    "signer_run_attempt": context["run_attempt"],
    "receipt_sha256": context["receipt_sha256"],
    "linux_payload_name": "velnor-actions-x86_64-unknown-linux-gnu",
    "linux_payload_sha256": context["subjects"][1]["digest"]["sha256"],
    "darwin_payload_name": "velnor-actions-aarch64-apple-darwin",
    "darwin_payload_sha256": context["subjects"][2]["digest"]["sha256"],
}

def require_verified_subject(subject, expected):
    results = verifier._invoke_gh(
        gh,
        subject,
        bundle_path,
        context["source_sha"],
        authority,
        policy,
        time.monotonic() + 90,
    )
    assert isinstance(results, list) and len(results) == 1
    statement = results[0]["verificationResult"]["statement"]
    assert statement["predicateType"] == "https://slsa.dev/provenance/v1"
    subjects = statement["subject"]
    assert len(subjects) == 3
    assert {(item["name"], item["digest"]["sha256"]) for item in subjects} == expected

with tempfile.TemporaryDirectory(prefix="source-preview-dsse-") as temporary:
    temporary = Path(temporary)
    receipt_path = temporary / "SDK_PREVIEW_RECEIPT.json"
    bundle_path = temporary / "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"
    receipt_path.write_bytes(receipt)
    bundle_path.write_bytes(bundle)
    expected = {
        ("SDK_PREVIEW_RECEIPT.json", context["subjects"][0]["digest"]["sha256"]),
        (preview["linux_payload_name"], preview["linux_payload_sha256"]),
        (preview["darwin_payload_name"], preview["darwin_payload_sha256"]),
    }
    for name in ("SDK_PREVIEW_RECEIPT.json", preview["linux_payload_name"], preview["darwin_payload_name"]):
        require_verified_subject(fixture / name, expected)
    verifier._verify_attestation(
        gh, receipt, bundle, preview, temporary, authority, policy, time.monotonic() + 120
    )
    changed = copy.deepcopy(preview)
    changed["linux_payload_sha256"] = "0" * 64
    try:
        verifier._verify_attestation(
            gh, receipt, bundle, changed, temporary, authority, policy, time.monotonic() + 120
        )
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("mismatched Linux subject digest was accepted")
