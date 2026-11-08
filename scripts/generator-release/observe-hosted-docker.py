#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
import pathlib
import re
import selectors
import signal
import time
import stat
import subprocess
import tempfile

DAEMON_FORMAT = '{"ID":{{json .ID}},"ServerVersion":{{json .ServerVersion}},"OSType":{{json .OSType}},"Architecture":{{json .Architecture}},"OperatingSystem":{{json .OperatingSystem}}}'


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate daemon field")
        result[key] = value
    return result


def probe(cli, args, env):
    process = subprocess.Popen([str(cli), *args], env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    selector = selectors.DefaultSelector()
    buffers = {process.stdout: bytearray(), process.stderr: bytearray()}
    deadline = time.monotonic() + 20
    try:
        for stream in buffers:
            selector.register(stream, selectors.EVENT_READ)
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Docker probe timed out")
            for key, _ in selector.select(remaining):
                chunk = os.read(key.fileobj.fileno(), 4097)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                buffer = buffers[key.fileobj]
                buffer.extend(chunk)
                if len(buffer) > 4096:
                    raise ValueError("Docker probe output exceeds bound")
        if process.wait(timeout=max(0.001, deadline - time.monotonic())) != 0:
            raise ValueError("Docker probe failed")
        stdout = bytes(buffers[process.stdout])
        if buffers[process.stderr] or b"\0" in stdout:
            raise ValueError("invalid probe output")
        return stdout.decode("utf-8").rstrip("\r\n")
    finally:
        selector.close()
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()



def main():
    if os.environ.get("GITHUB_EVENT_NAME") != "workflow_dispatch":
        raise ValueError("manual dispatch required")
    if os.environ.get("RUNNER_OS") != "Linux" or os.environ.get("RUNNER_ARCH") != "X64" or os.environ.get("ImageOS") != "ubuntu26":
        raise ValueError("Ubuntu 26.04 x86-64 runner required")
    workflow_sha = os.environ.get("GITHUB_WORKFLOW_SHA", "")
    source_sha = os.environ.get("GITHUB_SHA", "")
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    workflow_ref = os.environ.get("GITHUB_WORKFLOW_REF", "")
    if (source_sha != workflow_sha or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository)
            or not workflow_ref.startswith(repository + "/.github/workflows/source-qualification.yml@refs/heads/")
            or len(workflow_ref) > 512 or any(ord(c) < 33 or ord(c) > 126 for c in workflow_ref)):
        raise ValueError("invalid source or workflow identity")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    image = os.environ.get("ImageVersion", "")
    if (not re.fullmatch(r"[0-9a-f]{40}", workflow_sha)
            or not re.fullmatch(r"[1-9][0-9]*", run_id)
            or not re.fullmatch(r"[1-9][0-9]*", attempt)
            or not re.fullmatch(r"[0-9.]+", image)):
        raise ValueError("invalid workflow or image identity")
    cli = pathlib.Path("/usr/bin/docker")
    socket = pathlib.Path("/run/docker.sock")
    endpoint_socket = pathlib.Path("/var/run/docker.sock")
    descriptor = os.open(cli, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > 256 * 1024 * 1024:
            raise ValueError("invalid Docker CLI")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        sock = socket.lstat()
        endpoint_stat = endpoint_socket.stat()
        if (sock.st_dev, sock.st_ino) != (endpoint_stat.st_dev, endpoint_stat.st_ino) or not stat.S_ISSOCK(sock.st_mode):
            raise ValueError("invalid Docker socket")
        with tempfile.TemporaryDirectory(prefix="velnor-docker-observe-", dir="/tmp") as home:
            env = {"PATH": "/usr/bin:/bin", "HOME": home, "DOCKER_CONFIG": home, "LANG": "C.UTF-8"}
            workspace = os.environ.get("GITHUB_WORKSPACE", "")
            git_env = {**env, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_NO_REPLACE_OBJECTS": "1"}
            git = pathlib.Path("/usr/bin/git")
            script = "scripts/generator-release/observe-hosted-docker.py"
            if not workspace.startswith("/") or probe(git, ["-C", workspace, "rev-parse", "HEAD"], git_env) != source_sha:
                raise ValueError("checkout source identity mismatch")
            expected_blob = probe(git, ["-C", workspace, "rev-parse", source_sha + ":" + script], git_env)
            actual_blob = probe(git, ["-C", workspace, "hash-object", "--no-filters", script], git_env)
            if expected_blob != actual_blob or not re.fullmatch(r"[0-9a-f]{40}", expected_blob):
                raise ValueError("probe source identity mismatch")
            version = probe(cli, ["--version"], env)
            if not re.fullmatch(r"Docker version [0-9.]+, build [0-9a-f]+", version):
                raise ValueError("invalid Docker CLI identity")
            endpoint = probe(cli, ["context", "inspect", "default", "--format", "{{.Endpoints.docker.Host}}"], env)
            if endpoint != "unix:///var/run/docker.sock":
                raise ValueError("default Docker endpoint mismatch")
            daemon = json.loads(probe(cli, ["--host", "unix:///run/docker.sock", "info", "--format", DAEMON_FORMAT], env), object_pairs_hook=unique)
        if set(daemon) != {"ID", "ServerVersion", "OSType", "Architecture", "OperatingSystem"}:
            raise ValueError("invalid daemon identity")
        if (not re.fullmatch(r"[0-9]+(?:\.[0-9]+){2}", daemon["ServerVersion"])
                or daemon["OSType"] != "linux" or daemon["Architecture"] not in ("x86_64", "amd64")
                or any(not isinstance(value, str) or not value or len(value) > 256
                       or any(ord(c) < 32 or ord(c) > 126 for c in value) for value in daemon.values())):
            raise ValueError("unsupported daemon identity")
        os.lseek(descriptor, 0, os.SEEK_SET)
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            digest_after = hashlib.file_digest(stream, "sha256").hexdigest()
        after = os.fstat(descriptor)
        path_after = cli.lstat()
        sock_after = socket.lstat()
        endpoint_after = endpoint_socket.stat()
        if ((endpoint_after.st_dev, endpoint_after.st_ino) != (sock_after.st_dev, sock_after.st_ino)):
            raise ValueError("Docker endpoint alias changed during observation")
        if (digest != digest_after or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                or (before.st_dev, before.st_ino) != (path_after.st_dev, path_after.st_ino)
                or (sock.st_dev, sock.st_ino, sock.st_uid) != (sock_after.st_dev, sock_after.st_ino, sock_after.st_uid)):
            raise ValueError("Docker identity changed during observation")
        daemon_id = hashlib.sha256(daemon.pop("ID").encode()).hexdigest()
        result = {"schema": 1, "workflow_sha": workflow_sha, "workflow_ref": workflow_ref, "repository": repository, "run_id": run_id, "run_attempt": attempt,
                  "runner_image": "ubuntu-26.04", "runner_image_version": image,
                  "cli_path": str(cli), "cli_sha256": digest, "cli_version": version,
                  "observed_context": "default", "observed_context_endpoint": endpoint, "canonical_socket_path": str(socket), "socket_uid": sock.st_uid,
                  "daemon": daemon, "daemon_id_sha256": daemon_id, "authority": "observation-only"}
        print(json.dumps(result, sort_keys=True, separators=(",", ":")))
    finally:
        os.close(descriptor)


if __name__ == "__main__":
    main()
