"""Admit workflow origin from current-run APIs; external review remains separate.

API field shapes follow GitHub's workflow-run, workflow and repository REST docs.
An execution receipt proves the checked origin, never approval or qualification.
"""

import os
import re

from owned_tool_source import digest, strict_json


REPOSITORY = "tailrocks/velnor-new"
PREFIX = "repos/" + REPOSITORY
WORKFLOW_PATH = ".github/workflows/owned-tools.yml"
CANDIDATE_BRANCH = "owned-tool-candidates"
MAX_API = 8 * 1024 * 1024
ENVIRONMENT_FIELDS = {"GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "GITHUB_REF",
                      "GITHUB_EVENT_NAME", "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA"}
RECEIPT_FIELDS = {"schema", "status", "repository", "repository_id", "run_id", "run_attempt",
                  "commit", "event", "ref", "head_branch", "default_branch", "workflow", "api_sha256"}
API_EVIDENCE_FILES = {"repository": "execution-repository.json", "run": "execution-run.json",
                      "workflow": "execution-workflow.json"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def positive(value):
    return type(value) is int and value > 0


def hash_value(value, length):
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{" + str(length) + r"}", value) \
        and value != "0" * length


def environment_identity(environment):
    values = {key: environment.get(key, "") for key in ENVIRONMENT_FIELDS}
    require(all(isinstance(value, str) for value in values.values()), "invalid workflow environment types")
    require(all(re.fullmatch(r"[1-9][0-9]*", values[key])
                for key in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT")), "invalid current run identity")
    require(hash_value(values["GITHUB_SHA"], 40) and
            values["GITHUB_WORKFLOW_SHA"] == values["GITHUB_SHA"],
            "workflow source must equal exact event commit")
    require(values["GITHUB_WORKFLOW_REF"] == REPOSITORY + "/" + WORKFLOW_PATH + "@" + values["GITHUB_REF"],
            "workflow reference must identify exact repository/path/ref")
    route = (values["GITHUB_EVENT_NAME"], values["GITHUB_REF"])
    require(route in (("workflow_dispatch", "refs/heads/main"),
                      ("push", "refs/heads/" + CANDIDATE_BRANCH)),
            "execution event/reference is outside closed candidate routes")
    return values


def default_branch_admission(event, branch, default):
    require(default == "main", "canonical repository default branch must remain main")
    if event == "workflow_dispatch":
        require(branch == default, "dispatch must execute canonical default branch")
    elif event == "push":
        require(branch == CANDIDATE_BRANCH and branch != default,
                "candidate push must use literal non-default branch")
    else:
        raise ValueError("unsupported source qualification event")


def read_document(read_api, endpoint):
    require(endpoint == PREFIX or re.fullmatch(
        re.escape(PREFIX) + r"/actions/(?:runs|workflows)/[1-9][0-9]*", endpoint),
        "execution admission requires fixed repository read endpoints")
    raw = read_api(endpoint)
    require(isinstance(raw, bytes) and 0 < len(raw) <= MAX_API, "invalid or oversized execution API bytes")
    data = strict_json(raw)
    require(isinstance(data, dict), "invalid execution API object")
    return data, digest(raw), raw


def repository_identity(repository, canonical_id):
    require(isinstance(repository, dict) and repository.get("full_name") == REPOSITORY and
            positive(repository.get("id")) and repository["id"] == canonical_id,
            "execution repository must equal canonical repository identity")


def admit_execution(read_api, environment=None, api_documents=None):
    require(api_documents is None or isinstance(api_documents, dict) and not api_documents,
            "API evidence destination must be an empty mapping")
    env = environment_identity(os.environ if environment is None else environment)
    repository, repository_sha, repository_raw = read_document(read_api, PREFIX)
    require(repository.get("full_name") == REPOSITORY and positive(repository.get("id")),
            "canonical repository API identity mismatch")
    run, run_sha, run_raw = read_document(read_api, PREFIX + "/actions/runs/" + env["GITHUB_RUN_ID"])
    require(positive(run.get("id")) and run["id"] == int(env["GITHUB_RUN_ID"]) and
            positive(run.get("run_attempt")) and run["run_attempt"] == int(env["GITHUB_RUN_ATTEMPT"]),
            "current API run/attempt differs from job identity")
    repository_identity(run.get("repository"), repository["id"])
    repository_identity(run.get("head_repository"), repository["id"])
    branch = env["GITHUB_REF"][len("refs/heads/"):]
    require(run.get("head_sha") == env["GITHUB_SHA"] and run.get("event") == env["GITHUB_EVENT_NAME"] and
            run.get("head_branch") == branch, "current API commit/event/head branch mismatch")
    default_branch_admission(run["event"], branch, repository.get("default_branch"))
    require(run.get("path") in (WORKFLOW_PATH, WORKFLOW_PATH + "@" + branch),
            "current API run path must identify exact workflow source")
    workflow_id = run.get("workflow_id")
    require(positive(workflow_id), "invalid current API workflow identity")
    workflow, workflow_sha, workflow_raw = read_document(read_api, PREFIX + "/actions/workflows/" + str(workflow_id))
    require(positive(workflow.get("id")) and workflow["id"] == workflow_id and
            workflow.get("path") == WORKFLOW_PATH, "API workflow id/path mismatch")
    receipt = {"schema": 1, "status": "SOURCE_QUALIFICATION_EXECUTION_ADMITTED",
               "repository": REPOSITORY, "repository_id": repository["id"],
               "run_id": env["GITHUB_RUN_ID"], "run_attempt": env["GITHUB_RUN_ATTEMPT"],
               "commit": env["GITHUB_SHA"], "event": run["event"], "ref": env["GITHUB_REF"],
               "head_branch": branch, "default_branch": repository["default_branch"],
               "workflow": {"id": workflow_id, "path": WORKFLOW_PATH,
                            "ref": env["GITHUB_WORKFLOW_REF"], "sha": env["GITHUB_WORKFLOW_SHA"]},
               "api_sha256": {"repository": repository_sha, "run": run_sha, "workflow": workflow_sha}}
    validate_execution_receipt(receipt, env)
    if api_documents is not None:
        api_documents.update(repository=repository_raw, run=run_raw, workflow=workflow_raw)
    return receipt


def validate_execution_evidence(receipt, documents, environment=None):
    validate_execution_receipt(receipt, environment)
    require(isinstance(documents, dict) and set(documents) == set(API_EVIDENCE_FILES),
            "unexpected durable execution API evidence map")
    endpoints = {PREFIX: "repository", PREFIX + "/actions/runs/" + receipt["run_id"]: "run",
                 PREFIX + "/actions/workflows/" + str(receipt["workflow"]["id"]): "workflow"}
    def read_evidence(endpoint):
        require(endpoint in endpoints, "durable API workflow identity differs from execution receipt")
        return documents[endpoints[endpoint]]
    reconstructed = admit_execution(read_evidence, environment)
    require(reconstructed == receipt, "durable execution API evidence differs from receipt")
    return receipt


def validate_execution_receipt(receipt, environment=None):
    env = environment_identity(os.environ if environment is None else environment)
    require(isinstance(receipt, dict) and set(receipt) == RECEIPT_FIELDS and
            type(receipt["schema"]) is int and receipt["schema"] == 1 and
            receipt["status"] == "SOURCE_QUALIFICATION_EXECUTION_ADMITTED",
            "unexpected source execution receipt fields/status")
    require(receipt["repository"] == REPOSITORY and positive(receipt["repository_id"]) and
            receipt["run_id"] == env["GITHUB_RUN_ID"] and receipt["run_attempt"] == env["GITHUB_RUN_ATTEMPT"] and
            receipt["commit"] == env["GITHUB_SHA"] and receipt["event"] == env["GITHUB_EVENT_NAME"] and
            receipt["ref"] == env["GITHUB_REF"] and receipt["head_branch"] == env["GITHUB_REF"][len("refs/heads/"):],
            "source execution receipt differs from exact current job identity")
    default_branch_admission(receipt["event"], receipt["head_branch"], receipt["default_branch"])
    workflow = receipt["workflow"]
    require(isinstance(workflow, dict) and set(workflow) == {"id", "path", "ref", "sha"} and
            positive(workflow["id"]) and workflow["path"] == WORKFLOW_PATH and
            workflow["ref"] == env["GITHUB_WORKFLOW_REF"] and workflow["sha"] == env["GITHUB_SHA"],
            "source execution workflow receipt mismatch")
    hashes = receipt["api_sha256"]
    require(isinstance(hashes, dict) and set(hashes) == {"repository", "run", "workflow"} and
            all(hash_value(value, 64) for value in hashes.values()), "source execution API byte digests missing")
    return receipt
