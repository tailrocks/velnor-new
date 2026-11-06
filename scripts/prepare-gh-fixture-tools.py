#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import importlib.util
import json
import os
import pathlib
import platform
import stat
import subprocess
import sys
import tempfile


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate manifest key")
        result[key] = value
    return result


def copy_verified(source, destination, expected):
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= 128 * 1024 * 1024:
            raise ValueError("fixture tool is not a bounded regular file")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read(128 * 1024 * 1024 + 1)
        if len(data) != metadata.st_size or hashlib.sha256(data).hexdigest() != expected:
            raise ValueError("fixture tool digest mismatch")
    finally:
        os.close(fd)
    handle, name = tempfile.mkstemp(dir=destination.parent, prefix=".tool-")
    try:
        with os.fdopen(handle, "wb") as stream:
            stream.write(data)
        os.chmod(name, 0o500)
        os.replace(name, destination)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def main():
    if sys.argv[1:] not in ([], ["--with-image-tuf"]):
        raise ValueError("unsupported fixture staging arguments")
    with_image_tuf = bool(sys.argv[1:])
    if sys.platform != "linux":
        raise ValueError("CI fixture staging requires Linux")
    arch = {"x86_64": "x86_64", "aarch64": "aarch64"}.get(platform.machine())
    if arch is None:
        raise ValueError("unsupported fixture architecture")
    root = pathlib.Path(__file__).resolve().parents[1]
    manifest_path = root / "crates/velnor-actions-cli/tests/api2026_pull_context_fixture/tools.json"
    if manifest_path.stat().st_size > 16384:
        raise ValueError("fixture tool manifest too large")
    manifest = json.loads(manifest_path.read_bytes(), object_pairs_hook=unique)
    entry = manifest[arch]
    platforms = {"x86_64": ("linux_amd64", "linux/amd64"), "aarch64": ("linux_arm64", "linux/arm64")}
    if (entry["tool_platform"], entry["container_platform"]) != platforms[arch]:
        raise ValueError("fixture platform mismatch")
    for key in ("gh_sha256", "nextest_sha256"):
        value = entry[key]
        if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            raise ValueError("invalid fixture tool digest")
    temp = pathlib.Path(os.environ["RUNNER_TEMP"])
    if not temp.is_absolute() or not temp.is_dir():
        raise ValueError("runner temporary directory is unavailable")
    destination = temp / "velnor-gh-fixture-tools"
    destination.mkdir(mode=0o700, exist_ok=True)
    metadata = destination.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
        raise ValueError("fixture tool destination is not private")
    env = {key: os.environ[key] for key in ("PATH", "HOME", "MISE_DATA_DIR", "MISE_CACHE_DIR", "MISE_STATE_DIR") if key in os.environ}
    env.update({"MISE_AUTO_INSTALL": "0", "MISE_NO_CONFIG": "1", "MISE_NO_ENV": "1", "MISE_NO_HOOKS": "1"})
    def installed(spec):
        result = subprocess.run(["mise", "--no-config", "--no-env", "--no-hooks", "where", spec], env=env, check=True, capture_output=True, text=True, timeout=30)
        path = pathlib.Path(result.stdout.strip())
        if not path.is_absolute():
            raise ValueError("mise returned a relative path")
        return path
    gh_version = manifest["gh_version"]
    nextest_version = manifest["nextest_version"]
    if not all(isinstance(value, str) and value and all(c in "0123456789." for c in value) for value in (gh_version, nextest_version)):
        raise ValueError("invalid fixture tool version")
    gh = installed(f"gh@{gh_version}") / f"gh_{gh_version}_{entry['tool_platform']}" / "bin/gh"
    nextest = installed(f"aqua:nextest-rs/nextest/cargo-nextest@{nextest_version}") / "cargo-nextest"
    staged_gh = destination / f"gh_{entry['tool_platform']}"
    copy_verified(gh, staged_gh, entry["gh_sha256"])
    copy_verified(nextest, destination / f"cargo-nextest_{entry['tool_platform']}", entry["nextest_sha256"])
    if with_image_tuf:
        subprocess.run([sys.executable, str(root / "scripts/prepare-image-attestation-tuf.py"), "--gh", str(staged_gh), "--cache-root", str(destination / "image-attestation-tuf")], check=True, timeout=900)
    image = manifest["image"]
    if not isinstance(image, str) or not image.startswith("rust@sha256:") or len(image) != 76 or any(c not in "0123456789abcdef" for c in image[12:]):
        raise ValueError("invalid fixture image digest")
    sys.dont_write_bytecode = True
    helper_path = root / "crates/velnor-actions-cli/tests/api2026_pull_context_fixture/docker_host.py"
    spec = importlib.util.spec_from_file_location("velnor_fixture_docker_host", helper_path)
    if spec is None or spec.loader is None:
        raise ValueError("fixture Docker helper unavailable")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    docker = helper.trusted_docker()
    docker_env = helper.docker_environment()
    helper.local_daemon(docker, docker_env)
    subprocess.run([docker, "pull", "--platform", entry["container_platform"], image], env=docker_env, check=True, timeout=600, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    helper.verify_image(docker, docker_env, image)


if __name__ == "__main__":
    try:
        main()
    except (OSError, KeyError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(f"fixture tool staging failed: {error}") from None
