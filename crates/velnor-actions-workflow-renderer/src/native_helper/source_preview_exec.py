# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

import source_preview_bootstrap as bootstrap


COMMON_ENV = frozenset({
    "PATH", "HOME", "TMPDIR", "RUNNER_TEMP", "CI", "LANG", "LC_ALL",
    "GITHUB_WORKSPACE", "GITHUB_REPOSITORY", "GITHUB_REPOSITORY_ID",
    "GITHUB_EVENT_NAME", "GITHUB_EVENT_PATH", "GITHUB_REF", "GITHUB_REF_PROTECTED",
    "GITHUB_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_WORKFLOW_REF",
    "GITHUB_WORKFLOW_SHA", "GITHUB_JOB", "GITHUB_OUTPUT", "GITHUB_ENV",
    "VELNOR_RUN_KEY_ATTEMPT", "VELNOR_INTERNAL_OP",
})
TOOL_ENV = frozenset({
    "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_AUTO_INSTALL", "MISE_CARGO_HOME",
    "MISE_RUSTUP_HOME", "MISE_NO_CONFIG", "MISE_NO_ENV", "MISE_NO_HOOKS",
    "MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL", "MISE_LOCKFILE",
})
OPERATION_ENV = {
    "plan-v1": TOOL_ENV | {"GH_TOKEN", "VELNOR_REQUEST_FILE", "VELNOR_PLAN_MATRIX_OUTPUT_MODE"},
    "merge-v1": {"GH_TOKEN", "VELNOR_REQUEST_FILE", "VELNOR_NEEDS_JSON", "VELNOR_NEEDS_EXPECTED"},
    "fetch-reports-v1": {"GH_TOKEN", "GH_REPO"},
    "resolve-qualification-v1": {"GH_TOKEN", "VELNOR_REQUEST_FILE"},
    "write-request-v1": {"VELNOR_REQUEST_FILE", "VELNOR_NEEDS_JSON", "VELNOR_NEEDS_EXPECTED", "VELNOR_NAMED_CHECK_LANES_JSON"},
    "assemble-group-ledgers-v1": set(),
    "emit-group-ledger-v1": {"VELNOR_JOB_STATUS"},
    "execute-check-v1": TOOL_ENV | {"VELNOR_CHECK_ID", "VELNOR_CHECK_JOB_ID", "VELNOR_CHECK_LANE_VARIANT", "VELNOR_TASK_ID"},
    "execute-native-homebrew-check-v1": TOOL_ENV | {"GH_TOKEN", "VELNOR_CHECK_ID", "VELNOR_NATIVE_CHECK_OPERATION", "VELNOR_TASK_ID"},
    "resolve-task-execution-v1": TOOL_ENV | {"VELNOR_TASK_EXECUTION_DIGEST", "VELNOR_GENERATOR_VERSION", "VELNOR_RUNTIME_RUNNER_TEMP"},
    "write-task-report-v1": {"VELNOR_TASK_ID", "VELNOR_EXIT_CODE", "VELNOR_DOWNSTREAM_TASK_IDS", "VELNOR_START_MS", "VELNOR_UNIT_OBSERVATION_JSON", "VELNOR_NOT_SELECTED_REASON", "VELNOR_NOOP_TASK_DIGEST"},
    "repo-policy-v1": set(),
}


def restricted_environment(environment, identity):
    operation = environment.get("VELNOR_INTERNAL_OP")
    bootstrap.require(operation in OPERATION_ENV)
    allowed = COMMON_ENV | OPERATION_ENV[operation]
    result = {key: value for key, value in environment.items() if key in allowed}
    result["GIT_CONFIG_NOSYSTEM"] = "1"
    result["GIT_TERMINAL_PROMPT"] = "0"
    if identity["channel"] == "source-preview":
        result["VELNOR_SELECTED_PREVIEW_IDENTITY"] = json.dumps(identity, sort_keys=True, separators=(",", ":"))
    return result


def sealed_controller(context, network):
    bootstrap.require(sys.platform == "linux")
    expected = bootstrap.verify_controller(context, network)
    source = Path(context["runner_temp"]) / "velnor" / "bin" / "controller" / "binary"
    source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    sealed_fd = -1
    try:
        metadata = os.fstat(source_fd)
        bootstrap.require(stat.S_ISREG(metadata.st_mode) and 0 < metadata.st_size <= bootstrap.MAX_PAYLOAD)
        sealed_fd = os.memfd_create("velnor-approved-controller", os.MFD_ALLOW_SEALING)
        remaining = bootstrap.MAX_PAYLOAD + 1
        while remaining:
            chunk = os.read(source_fd, min(65_536, remaining))
            if not chunk:
                break
            remaining -= len(chunk)
            offset = 0
            while offset < len(chunk):
                offset += os.write(sealed_fd, chunk[offset:])
        bootstrap.require(remaining > 0)
        seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(sealed_fd, fcntl.F_ADD_SEALS, seals)
        bootstrap.require(fcntl.fcntl(sealed_fd, fcntl.F_GET_SEALS) == seals)
        os.lseek(sealed_fd, 0, os.SEEK_SET)
        digest = hashlib.sha256()
        while chunk := os.read(sealed_fd, 65_536):
            digest.update(chunk)
        bootstrap.require(digest.hexdigest() == expected["sha256"])
        result = sealed_fd
        sealed_fd = -1
        return result, expected
    finally:
        os.close(source_fd)
        if sealed_fd >= 0:
            os.close(sealed_fd)


def execute_controller(context, network, arguments, environment):
    bootstrap.require(arguments == ["velnor-actions"])
    bootstrap.require(environment.get("VELNOR_INTERNAL_OP") in OPERATION_ENV)
    sealed_fd, identity = sealed_controller(context, network)
    try:
        child_environment = restricted_environment(environment, identity)
        os.execve(f"/proc/self/fd/{sealed_fd}", arguments, child_environment)
    finally:
        os.close(sealed_fd)


def main():
    bootstrap.require(len(sys.argv) == 1 and os.environ.get("VELNOR_INTERNAL_OP") in OPERATION_ENV)
    context = bootstrap.environment_context()
    network = bootstrap.GitHubTransport(os.environ.get("GH_TOKEN", ""))
    path = Path(context["runner_temp"]) / "velnor" / "bin" / "controller" / "binary"
    if not path.exists():
        bootstrap.authorize_and_acquire(context, network)
    execute_controller(context, network, ["velnor-actions"], os.environ)


if __name__ == "__main__":
    try:
        main()
    except (bootstrap.AdmissionError, OSError, ValueError, KeyError, subprocess.SubprocessError):
        raise SystemExit("source preview execution rejected") from None
