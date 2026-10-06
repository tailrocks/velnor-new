"""Unsigned isolated execution-evidence fixtures layered on measured-byte fixtures."""

import json

from owned_tool_publication_test_fixtures import P, archive, attestation, binary, receipt_bytes
from owned_tool_publication_test_fixtures import fixture as byte_fixture
import owned_tool_qualification_evidence as Q
import source_qualification_execution as E


def environment(manifest, route="main"):
    event, reference = (("workflow_dispatch", "refs/heads/main") if route == "main" else
                        ("push", "refs/heads/owned-tool-candidates"))
    return {"GITHUB_EVENT_NAME": event, "GITHUB_REF": reference,
        "GITHUB_REPOSITORY": E.REPOSITORY, "GITHUB_SHA": manifest["workflow_commit"],
        "GITHUB_RUN_ID": manifest["workflow"]["run_id"],
        "GITHUB_RUN_ATTEMPT": manifest["workflow"]["run_attempt"],
        "GITHUB_WORKFLOW_REF": E.REPOSITORY + "/" + E.WORKFLOW_PATH + "@" + reference,
        "GITHUB_WORKFLOW_SHA": manifest["workflow_commit"]}


def execution(manifest, target, route):
    env = environment(manifest, route)
    repository = {"id": 7, "full_name": E.REPOSITORY, "default_branch": "main"}
    run = {"id": int(env["GITHUB_RUN_ID"]), "run_attempt": int(env["GITHUB_RUN_ATTEMPT"]),
        "repository": repository, "head_repository": repository,
        "head_sha": env["GITHUB_SHA"], "event": env["GITHUB_EVENT_NAME"],
        "head_branch": env["GITHUB_REF"].removeprefix("refs/heads/"),
        "path": E.WORKFLOW_PATH, "workflow_id": 20, "observed_host": target}
    documents = {"repository": Q.receipt_bytes(repository), "run": Q.receipt_bytes(run),
                 "workflow": Q.receipt_bytes({"id": 20, "path": E.WORKFLOW_PATH})}
    endpoints = {E.PREFIX: "repository", E.PREFIX + "/actions/runs/" + env["GITHUB_RUN_ID"]: "run",
                 E.PREFIX + "/actions/workflows/20": "workflow"}
    origin = E.admit_execution(lambda endpoint: documents[endpoints[endpoint]], env)
    return origin, documents


def fixture(tool="mise", route="main"):
    manifest, files = byte_fixture(tool)
    for artifact in manifest["artifacts"]:
        target, claim = artifact["target"], artifact["qualification"]
        name = "qualified-receipt-" + target + ".json"
        receipt = json.loads(files[name])
        behavior = receipt["behavioral_qualification"]
        origin, documents = execution(manifest, target, route)
        admission = behavior["artifact_admission"]
        admission["execution"] = origin
        behavior["execution_evidence"] = {"directory": "execution-" + target,
                                          "api_sha256": origin["api_sha256"]}
        behavior["artifact_admission_sha256"] = Q.digest(Q.receipt_bytes(admission))
        files[name] = Q.receipt_bytes(receipt)
        claim["qualified_receipt_sha256"] = Q.digest(files[name])
        claim["sourceartifact_execution_sha256"] = Q.digest(Q.receipt_bytes(origin))
        for role, filename in E.API_EVIDENCE_FILES.items():
            files["execution-" + target + "/" + filename] = documents[role]
    return manifest, files
