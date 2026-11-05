# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import grp
import os
import pathlib
import stat
import subprocess
import sys

def trusted_docker():
    if sys.platform == "darwin":
        candidates = [pathlib.Path.home() / ".orbstack/bin/docker", pathlib.Path("/usr/local/bin/docker"), pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")]
        roots = [pathlib.Path("/Applications/OrbStack.app/Contents"), pathlib.Path("/Applications/Docker.app/Contents")]
    elif sys.platform.startswith("linux"):
        candidates = [pathlib.Path("/usr/bin/docker"), pathlib.Path("/usr/local/bin/docker")]
        roots = [pathlib.Path("/usr/bin"), pathlib.Path("/usr/local/bin")]
    else:
        raise RuntimeError("unsupported fixture Docker host")
    for candidate in candidates:
        if not candidate.exists():
            continue
        resolved = candidate.resolve(strict=True)
        if not any(resolved.is_relative_to(root) for root in roots):
            continue
        metadata = resolved.stat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o022:
            continue
        owners = {0, os.getuid()} if sys.platform == "darwin" else {0}
        if metadata.st_uid not in owners:
            continue
        parents = set(candidate.parents) | set(resolved.parents)
        def untrusted_parent(path):
            metadata = path.stat()
            if not path.is_dir() or path.is_symlink() or metadata.st_uid not in owners or metadata.st_mode & 0o002:
                return True
            if metadata.st_mode & 0o020:
                return not (sys.platform == "darwin" and path == pathlib.Path("/Applications") and metadata.st_uid == 0 and grp.getgrgid(metadata.st_gid).gr_name == "admin")
            return False
        if any(untrusted_parent(path) for path in parents):
            continue
        return str(candidate)
    raise RuntimeError("trusted local Docker client is unavailable")


def docker_environment():
    return {"HOME": str(pathlib.Path.home()), "PATH": "/usr/bin:/bin"}



def local_daemon(client, environment):
    endpoint = subprocess.check_output([client, "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"], env=environment, text=True, timeout=15).strip()
    if len(endpoint) > 4096 or not endpoint.startswith("unix:///") or not stat.S_ISSOCK(pathlib.Path(endpoint[7:]).stat().st_mode):
        raise RuntimeError("fixture Docker daemon must use a local Unix socket")


def verify_image(client, environment, image):
    output = subprocess.check_output([client, "image", "inspect", "--format", "{{json .RepoDigests}}", image], env=environment, text=True, timeout=15)
    if len(output) > 65536:
        raise RuntimeError("fixture Docker image metadata exceeds its bound")
    digests = json.loads(output)
    if not isinstance(digests, list) or image not in digests:
        raise RuntimeError("fixture Docker image digest mismatch")
