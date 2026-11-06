#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import tempfile


def main():
    with tempfile.TemporaryDirectory() as work:
        root = pathlib.Path(work)
        scripts = root / "scripts"
        scripts.mkdir()
        shutil.copyfile(pathlib.Path(__file__).with_name("prepare-gh-fixture-tools.py"), scripts / "prepare-gh-fixture-tools.py")
        config = root / "crates/velnor-actions-cli/tests/api2026_pull_context_fixture"
        config.mkdir(parents=True)
        bindir = root / "bin"
        bindir.mkdir()
        arch = {"x86_64": ("x86_64", "linux_amd64", "linux/amd64"), "aarch64": ("aarch64", "linux_arm64", "linux/arm64")}[platform.machine()]
        arch_name, tool_platform, container_platform = arch
        gh_root = root / "gh"
        nextest_root = root / "nextest"
        gh = gh_root / f"gh_2.103.3_{tool_platform}/bin/gh"
        nextest = nextest_root / "cargo-nextest"
        gh.parent.mkdir(parents=True)
        nextest.parent.mkdir()
        gh.write_bytes(b"fixture-gh")
        nextest.write_bytes(b"fixture-nextest")
        entry = {"tool_platform": tool_platform, "container_platform": container_platform, "gh_sha256": hashlib.sha256(gh.read_bytes()).hexdigest(), "nextest_sha256": hashlib.sha256(nextest.read_bytes()).hexdigest()}
        manifest = {"gh_version": "2.103.3", "nextest_version": "0.9.151", "image": "rust@sha256:" + "a" * 64, arch_name: entry}
        (config / "tools.json").write_text(json.dumps(manifest))
        (config / "docker_host.py").write_text(
            "import os\ndef trusted_docker(): return os.environ['PATH'].split(':')[0] + '/docker'\n"
            "def docker_environment(): return {'PATH': os.environ['PATH']}\n"
            "def local_daemon(client, env): pass\n"
            "def verify_image(client, env, image): pass\n"
        )
        mise = bindir / "mise"
        mise.write_text(f'#!/bin/sh\ncase "$*" in *cargo-nextest*) printf "%s\\n" "{nextest_root}";; *) printf "%s\\n" "{gh_root}";; esac\n')
        docker = bindir / "docker"
        docker.write_text('#!/bin/sh\n[ "$1" = pull ]\n')
        mise.chmod(0o700)
        docker.chmod(0o700)
        temp = root / "runner"
        temp.mkdir()
        env = {"PATH": f"{bindir}:/usr/bin:/bin", "HOME": str(root), "RUNNER_TEMP": str(temp)}
        args = ["python3", "-I", str(scripts / "prepare-gh-fixture-tools.py")]
        invalid = subprocess.run([*args, "--unknown"], env=env, capture_output=True)
        assert invalid.returncode != 0
        assert b"unsupported fixture staging arguments" in invalid.stderr
        assert not (temp / "velnor-gh-fixture-tools").exists()
        subprocess.run(args, env=env, check=True, capture_output=True)
        dest = temp / "velnor-gh-fixture-tools"
        assert (dest / f"gh_{tool_platform}").read_bytes() == b"fixture-gh"
        assert (dest / f"cargo-nextest_{tool_platform}").read_bytes() == b"fixture-nextest"
        manifest[arch_name]["tool_platform"] = "linux_wrong"
        (config / "tools.json").write_text(json.dumps(manifest))
        assert subprocess.run(args, env=env, capture_output=True).returncode != 0
        manifest[arch_name]["tool_platform"] = tool_platform
        (config / "tools.json").write_text(json.dumps(manifest))
        gh.write_bytes(b"tampered")
        assert subprocess.run(args, env=env, capture_output=True).returncode != 0
        docker.unlink()
        gh.write_bytes(b"fixture-gh")
        assert subprocess.run(args, env=env, capture_output=True).returncode != 0


if __name__ == "__main__":
    main()
