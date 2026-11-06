# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import errno
import fcntl
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
sys.path.insert(0, str(root / "src/native_helper"))
spec = importlib.util.spec_from_file_location("preview_fixture", root / "tests/source_preview_bootstrap_scenarios.py")
fixture = importlib.util.module_from_spec(spec)
sys.argv = [str(root / "tests/source_preview_bootstrap_scenarios.py"), str(root / "src/native_helper/source_preview_bootstrap.py")]
spec.loader.exec_module(fixture)
import source_preview_bootstrap as bootstrap
import source_preview_exec as executor


with tempfile.TemporaryDirectory() as directory:
    temporary = Path(directory).resolve()
    repository = temporary / "repo"
    repository.mkdir()
    subprocess.run(["git", "init", "-q", str(repository)], check=True)
    subprocess.run(["git", "-C", str(repository), "-c", "user.name=Alexey Zhokhov", "-c", "user.email=alexey@zhokhov.com", "commit", "--allow-empty", "-s", "-qm", "fixture", "-m", "Co-authored-by: Codex <codex@openai.com>"], check=True)
    candidate = subprocess.check_output(["git", "-C", str(repository), "rev-parse", "HEAD"], text=True).strip()
    sdk = os.environ.get("VELNOR_EXEC_FIXTURE")
    fixture.PAYLOAD = Path(sdk or "/usr/bin/python3").read_bytes()
    expected_output = subprocess.check_output([sdk, "--version"]) if sdk else b"approved-controller\n"
    assert expected_output, "selected executable produced no version output"
    value, receipt = fixture.record()
    dot = repository / ".velnor"
    dot.mkdir()
    (dot / "generator.lock").write_bytes(fixture.lock(value, "planner-preview"))
    network = fixture.FakeGitHub(value, receipt, fixture.lock(value, "planner-preview-staged"))
    context = {"event": network.event, "event_name": "pull_request", "repository": bootstrap.REPOSITORY, "candidate": candidate, "ref": "refs/pull/7/merge", "workspace": str(repository), "lock_path": str(dot / "generator.lock"), "runner_temp": str(temporary)}
    path = Path(bootstrap.authorize_and_acquire(context, network))
    network.tag_reads = 0
    network.replace_after_tag_read = path
    try:
        executor.sealed_controller(context, network)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("unapproved controller replaced during sealing was accepted")
    network.replace_after_tag_read = None
    path.write_bytes(fixture.PAYLOAD)
    descriptor, _ = executor.sealed_controller(context, network)
    try:
        expected = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        if fcntl.fcntl(descriptor, fcntl.F_GET_SEALS) != expected:
            raise AssertionError("controller execution is not protected from later modification")
        try:
            os.write(descriptor, b"unapproved")
        except OSError as error:
            assert error.errno == errno.EPERM
        else:
            raise AssertionError("sealed controller accepted a write")
        path.unlink()
        path.write_bytes(b"unapproved replacement")
        read_end, write_end = os.pipe()
        child = os.fork()
        if child == 0:
            os.close(read_end)
            os.dup2(write_end, 1)
            os.close(write_end)
            args = ["velnor-actions", "--version"] if sdk else ["python3", "-c", 'print("approved-controller")']
            os.execve(f"/proc/self/fd/{descriptor}", args, {})
        os.close(write_end)
        result = os.read(read_end, 1024)
        os.close(read_end)
        _, status = os.waitpid(child, 0)
        assert os.waitstatus_to_exitcode(status) == 0 and result == expected_output
        try:
            executor.sealed_controller(context, network)
        except bootstrap.AdmissionError:
            pass
        else:
            raise AssertionError("unapproved replacement was accepted")
    finally:
        os.close(descriptor)


def environment_case(operation, expected_token, extra_arg=False):
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory).resolve()
        repository = temporary / "repo"
        repository.mkdir()
        subprocess.run(["git", "init", "-q", str(repository)], check=True)
        subprocess.run(["git", "-C", str(repository), "-c", "user.name=Alexey Zhokhov", "-c", "user.email=alexey@zhokhov.com", "commit", "--allow-empty", "-s", "-qm", "fixture", "-m", "Co-authored-by: Codex <codex@openai.com>"], check=True)
        candidate = subprocess.check_output(["git", "-C", str(repository), "rev-parse", "HEAD"], text=True).strip()
        fixture.PAYLOAD = Path("/usr/bin/python3").read_bytes()
        value, receipt = fixture.record()
        dot = repository / ".velnor"
        dot.mkdir()
        (dot / "generator.lock").write_bytes(fixture.lock(value, "planner-preview"))
        network = fixture.FakeGitHub(value, receipt, fixture.lock(value, "planner-preview-staged"))
        context = {"event": network.event, "event_name": "pull_request", "repository": bootstrap.REPOSITORY, "candidate": candidate, "ref": "refs/pull/7/merge", "workspace": str(repository), "lock_path": str(dot / "generator.lock"), "runner_temp": str(temporary)}
        bootstrap.authorize_and_acquire(context, network)
        environment = {"PATH": "/usr/bin:/bin", "VELNOR_INTERNAL_OP": operation, "GH_TOKEN": "read-token-canary", "GITHUB_TOKEN": "github-secret-canary", "ACTIONS_RUNTIME_TOKEN": "runtime-secret-canary", "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "oidc-secret-canary", "LD_PRELOAD": "loader-canary", "PYTHONPATH": "python-canary", "HTTPS_PROXY": "proxy-canary", "BASH_ENV": "shell-canary", "MISE_GITHUB_TOKEN": "mise-secret-canary", "CARGO_BUILD_RUSTC_WRAPPER": "wrapper-canary", "VELNOR_FORGED": "parameter-canary", "VELNOR_SELECTED_PREVIEW_IDENTITY": "forged"}
        if operation == "unknown-op" or extra_arg:
            read_end, write_end = os.pipe()
            child = os.fork()
            if child == 0:
                os.close(read_end)
                os.dup2(write_end, 1)
                os.close(write_end)
                try:
                    arguments = ["velnor-actions", "--help"] if extra_arg else ["velnor-actions"]
                    executor.execute_controller(context, network, arguments, environment)
                except bootstrap.AdmissionError:
                    os.write(1, b"REJECTED")
                    os._exit(0)
                os._exit(1)
            os.close(write_end)
            output = os.read(read_end, 16_384)
            os.close(read_end)
            _, status = os.waitpid(child, 0)
            assert os.waitstatus_to_exitcode(status) == 0 and output == b"REJECTED", "unapproved SDK operation or public command was executed"
            return
        read_end, write_end = os.pipe()
        input_read, input_write = os.pipe()
        child = os.fork()
        if child == 0:
            os.close(read_end)
            os.close(input_write)
            os.dup2(write_end, 1)
            os.dup2(input_read, 0)
            os.close(write_end)
            os.close(input_read)
            executor.execute_controller(context, network, ["velnor-actions"], environment)
        os.close(write_end)
        os.close(input_read)
        os.write(input_write, b"import os, json; print(json.dumps(dict(os.environ), sort_keys=True))\n")
        os.close(input_write)
        output = os.read(read_end, 16_384)
        os.close(read_end)
        _, status = os.waitpid(child, 0)
        assert os.waitstatus_to_exitcode(status) == 0
        assert b"read-token-canary" in output if expected_token else b"read-token-canary" not in output
        for forbidden in (b"github-secret-canary", b"runtime-secret-canary", b"oidc-secret-canary", b"loader-canary", b"python-canary", b"proxy-canary", b"shell-canary", b"mise-secret-canary", b"wrapper-canary", b"parameter-canary", b"forged"):
            assert forbidden not in output, "credential or untrusted runtime configuration reached SDK"
        assert b"VELNOR_SELECTED_PREVIEW_IDENTITY" in output


environment_case("write-task-report-v1", False)
environment_case("fetch-reports-v1", True)
environment_case("unknown-op", False)
environment_case("fetch-reports-v1", False, extra_arg=True)


def unknown_entrypoint_does_not_read_event():
    import time

    with tempfile.TemporaryDirectory() as directory:
        fifo = Path(directory) / "event"
        os.mkfifo(fifo)
        environment = {"PATH": "/usr/bin:/bin", "VELNOR_INTERNAL_OP": "unknown-op", "GITHUB_EVENT_PATH": str(fifo), "GITHUB_WORKSPACE": directory, "PYTHONDONTWRITEBYTECODE": "1"}
        child = subprocess.Popen(["/usr/bin/python3", str(root / "src/native_helper/source_preview_exec.py")], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=environment)
        observed_read = False
        deadline = time.monotonic() + 10
        try:
            while child.poll() is None and time.monotonic() < deadline:
                try:
                    writer = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
                except OSError as error:
                    if error.errno != errno.ENXIO:
                        raise
                    time.sleep(0.01)
                else:
                    observed_read = True
                    os.close(writer)
                    break
            timed_out = child.poll() is None and not observed_read
            if child.poll() is None:
                child.kill()
            status = child.wait(timeout=5)
            assert not timed_out and not observed_read and status != 0, "unknown SDK operation read event data or failed to reject promptly"
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()


unknown_entrypoint_does_not_read_event()
