# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(sys.argv[1]).resolve()
sdk_path = Path(sys.argv[2]).resolve()
sdk = sdk_path.read_bytes()
expected_version = sys.argv[3]
version = subprocess.run([str(sdk_path), "--version"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
assert version.returncode == 0 and version.stdout == f"velnor-actions {expected_version}\n".encode(), "selected SDK does not start with the expected version"
renderer = root / "crates/velnor-actions-workflow-renderer"
sys.path.insert(0, str(renderer / "src/native_helper"))
spec = importlib.util.spec_from_file_location("preview_external_fixture", renderer / "tests/source_preview_bootstrap_scenarios.py")
fixture = importlib.util.module_from_spec(spec)
sys.argv = [str(renderer / "tests/source_preview_bootstrap_scenarios.py"), str(renderer / "src/native_helper/source_preview_bootstrap.py")]
spec.loader.exec_module(fixture)
import source_preview_bootstrap as bootstrap
import source_preview_exec as executor

CONFIG = '''schema = 2
[workflow]
policy = "velnor-repository-v1"
default_branch = "main"
generator_validation = "bootstrap"
[discovery]
exclude = ["Cargo.toml"]
[stacks.rust]
test_runner = "cargo_nextest"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "fixture-scale-set"
labels = ["velnor", "fixture-scale-set"]
platform = "linux/amd64"
[[checks]]
id = "runtime-fixture"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
'''
RUN_KEY = "r260001-a1"

def git(repo, *args):
    result = subprocess.run(["/usr/bin/git", "-C", str(repo), *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, env={"PATH": "/usr/bin:/bin", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_TERMINAL_PROMPT": "0"})
    if result.returncode:
        raise AssertionError(result.stderr.decode())
    return result.stdout.decode().strip()


def selection(repo, temporary, payload, selected_source, receipt_variant=False, stable=False):
    fixture.PAYLOAD = payload
    value, receipt = fixture.record(selected_source)
    value["version"] = expected_version
    if receipt_variant:
        value["signer_run_attempt"] = 2
    receipt = json.dumps(bootstrap.receipt_from_record(value), sort_keys=True, separators=(",", ":")).encode()
    value["receipt_sha256"] = hashlib.sha256(receipt).hexdigest()
    base_lock = (root / ".velnor/generator.lock").read_bytes()
    lock = base_lock if stable else base_lock + b"\n" + fixture.lock(value, "planner-preview")
    (repo / ".velnor/generator.lock").write_bytes(lock)
    approved = lock
    if stable:
        digest = hashlib.sha256(payload).hexdigest()
        approved = ('schema = 1\n[generator]\nbinary = "velnor-actions"\nversion = "0.1.75"\ncommit = "' + "e" * 40 + '"\n[[generator.binaries]]\ntarget = "x86_64-unknown-linux-gnu"\nartifact = "https://github.com/' + bootstrap.REPOSITORY + '/releases/download/v0.1.75/velnor-actions-0.1.75-x86_64-unknown-linux-gnu"\nsha256 = "' + digest + '"\n').replace("0.1.75", expected_version).encode()
    network = fixture.FakeGitHub(value, receipt, approved)
    candidate = git(repo, "rev-parse", "HEAD")
    context = {"event": network.event, "event_name": "pull_request", "repository": bootstrap.REPOSITORY, "candidate": candidate, "ref": "refs/pull/7/merge", "workspace": str(repo), "lock_path": str(repo / ".velnor/generator.lock"), "runner_temp": str(temporary)}
    bootstrap.authorize_and_acquire(context, network)
    return context, network


def launch(context, network, repo, temporary, operation, extra):
    environment = {"PATH": "/usr/bin:/bin", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_TERMINAL_PROMPT": "0", "GITHUB_REPOSITORY": bootstrap.REPOSITORY, "GITHUB_WORKSPACE": str(repo), "RUNNER_TEMP": str(temporary), "GITHUB_RUN_ID": "260001", "GITHUB_RUN_ATTEMPT": "1", "GITHUB_OUTPUT": str(temporary / "github-output"), "VELNOR_INTERNAL_OP": operation, **extra}
    output = temporary / (operation + ".output")
    pid = os.fork()
    if pid == 0:
        try:
            descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            os.dup2(descriptor, 1)
            os.dup2(descriptor, 2)
            os.close(descriptor)
            os.chdir(repo)
            executor.execute_controller(context, network, ["velnor-actions"], environment)
        except BaseException as error:
            print(str(error), file=sys.stderr)
            os._exit(125)
    _, status = os.waitpid(pid, 0)
    return os.waitstatus_to_exitcode(status), output.read_text(errors="replace")


def prepare(repo, root_temp, label, payload, selected_source="a" * 40, receipt_variant=False, plan_bytes=None, stable=False):
    temporary = root_temp / label
    temporary.mkdir()
    context, network = selection(repo, temporary, payload, selected_source, receipt_variant, stable)
    if plan_bytes is not None:
        plan_path = temporary / "velnor" / RUN_KEY / "plan.json"
        plan_path.parent.mkdir(parents=True)
        plan_path.write_bytes(plan_bytes)
    return context, network, temporary


def merge_request(plan):
    return {"schema": 1, "run_key": RUN_KEY, "actual_event": "pull_request", "plan": plan, "matrix": plan["matrix"], "matrix_reports": [], "task_reports": [], "required_job_ids": ["plan"], "required_jobs": [{"job_id": "plan", "conclusion": "success"}]}


with tempfile.TemporaryDirectory() as directory:
    temporary = Path(directory).resolve()
    repo = temporary / "repo"
    repo.mkdir()
    git(repo, "init", "-b", "main")
    git(repo, "config", "user.name", "Alexey Zhokhov")
    git(repo, "config", "user.email", "alexey@zhokhov.com")
    git(repo, "config", "commit.gpgsign", "false")
    git(repo, "remote", "add", "origin", "https://github.com/tailrocks/velnor-new.git")
    (repo / "Cargo.toml").write_text('[package]\nname="preview-identity-fixture"\nversion="0.1.0"\nedition="2024"\n')
    (repo / "src").mkdir()
    (repo / "src/lib.rs").write_text("pub fn fixture() {}\n")
    git(repo, "add", "Cargo.toml", "src/lib.rs")
    git(repo, "commit", "-s", "-m", "preview identity fixture", "-m", "Co-authored-by: Codex <codex@openai.com>")
    base = git(repo, "rev-parse", "HEAD")
    with (repo / "Cargo.toml").open("a") as output:
        output.write("\n")
    git(repo, "add", "Cargo.toml")
    git(repo, "commit", "-s", "-m", "change preview fixture", "-m", "Co-authored-by: Codex <codex@openai.com>")
    head = git(repo, "rev-parse", "HEAD")
    fixture.BASE = base
    (repo / ".velnor").mkdir()
    (repo / ".velnor/config.toml").write_text(CONFIG)
    shutil.copy(root / ".velnor/controller-approvals.json", repo / ".velnor/controller-approvals.json")
    (repo / "mise.toml").write_text('[tasks.fixture-check]\nrun = "false"\n')
    first, network, plan_temp = prepare(repo, temporary, "plan", sdk)
    request = plan_temp / "plan-v1-request.json"
    request.write_text(json.dumps({"schema": 1, "op": "plan-v1", "run_key": RUN_KEY, "base": base, "head": head, "event": "pull_request", "root": str(repo)}))
    status, log = launch(first, network, repo, plan_temp, "plan-v1", {"VELNOR_REQUEST_FILE": str(request)})
    assert status == 0, "approved preview could not create Plan: " + log
    plan_bytes = (plan_temp / "velnor" / RUN_KEY / "plan.json").read_bytes()
    plan = json.loads(plan_bytes)
    assert plan["generator"]["sha256"] == hashlib.sha256(sdk).hexdigest()
    assert plan["generator"].get("preview") == {"channel": "source-preview", "source_sha": "a" * 40, "receipt_sha256": network.value["receipt_sha256"]}, "approved preview was omitted or changed in Plan"
    obligation = next(row for row in plan["obligations"] if row["job_id"] == "check-runtime-fixture")
    task_id = obligation["task_id"]
    for label, payload, selected_source, receipt_variant, match in [("matching", sdk, "a" * 40, False, True), ("different-sdk", sdk + b"\0", "a" * 40, False, False), ("different-source", sdk, "d" * 40, False, False), ("different-receipt", sdk, "a" * 40, True, False)]:
        context, network, run_temp = prepare(repo, temporary, label, payload, selected_source, receipt_variant, plan_bytes)
        status, log = launch(context, network, repo, run_temp, "write-task-report-v1", {"VELNOR_TASK_ID": task_id, "VELNOR_EXIT_CODE": "1"})
        reports = list((run_temp / "velnor" / RUN_KEY).glob("*/tasks/*.json"))
        if match:
            assert status == 0 and reports, "matching approved SDK did not write its failed-task report: " + log
        else:
            assert status != 0 and "preview_controller_identity_mismatch" in log and not reports, label + " wrote a task report: " + log
        if not match:
            merge_context, merge_network, merge_temp = prepare(repo, temporary, label + "-required", payload, selected_source, receipt_variant, plan_bytes)
            merge = merge_temp / "merge-v1-request.json"
            merge.write_text(json.dumps(merge_request(plan)))
            result, merge_log = launch(merge_context, merge_network, repo, merge_temp, "merge-v1", {"VELNOR_REQUEST_FILE": str(merge)})
            assert result != 0 and "preview_controller_identity_mismatch" in merge_log, label + " crossed Required: " + merge_log
    unresolved = copy.deepcopy(plan)
    unresolved["generator"]["sha256"] = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    unresolved_bytes = json.dumps(unresolved, sort_keys=True, separators=(",", ":")).encode()
    context, network, run_temp = prepare(repo, temporary, "unresolved", sdk, plan_bytes=unresolved_bytes)
    status, log = launch(context, network, repo, run_temp, "write-task-report-v1", {"VELNOR_TASK_ID": task_id, "VELNOR_EXIT_CODE": "1"})
    assert status != 0 and not list((run_temp / "velnor" / RUN_KEY).glob("*/tasks/*.json")), "unresolved Plan wrote a task report: " + log
    (repo / ".velnor/config.toml").write_text(CONFIG.split("[[checks]]")[0])
    context, network, no_work_temp = prepare(repo, temporary, "no-work-plan", sdk)
    request = no_work_temp / "plan-v1-request.json"
    request.write_text(json.dumps({"schema": 1, "op": "plan-v1", "run_key": RUN_KEY, "base": base, "head": head, "event": "pull_request", "root": str(repo)}))
    status, log = launch(context, network, repo, no_work_temp, "plan-v1", {"VELNOR_REQUEST_FILE": str(request)})
    assert status == 0, "approved preview could not create no-work Plan: " + log
    no_work_bytes = (no_work_temp / "velnor" / RUN_KEY / "plan.json").read_bytes()
    no_work = json.loads(no_work_bytes)
    assert not no_work["task_ids"] and not no_work["obligations"], "no-work Plan unexpectedly requires work"
    context, network, stable_plan_temp = prepare(repo, temporary, "stable-plan", sdk, stable=True)
    request = stable_plan_temp / "plan-v1-request.json"
    request.write_text(json.dumps({"schema": 1, "op": "plan-v1", "run_key": RUN_KEY, "base": base, "head": head, "event": "pull_request", "root": str(repo)}))
    status, log = launch(context, network, repo, stable_plan_temp, "plan-v1", {"VELNOR_REQUEST_FILE": str(request)})
    assert status == 0, "synthetic stable approval could not create Plan: " + log
    stable_bytes = (stable_plan_temp / "velnor" / RUN_KEY / "plan.json").read_bytes()
    stable_plan = json.loads(stable_bytes)
    assert stable_plan["generator"]["sha256"] == hashlib.sha256(sdk).hexdigest() and stable_plan["generator"].get("preview") is None
    assert not stable_plan["task_ids"] and not stable_plan["obligations"]
