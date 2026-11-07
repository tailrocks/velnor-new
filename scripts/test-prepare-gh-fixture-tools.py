#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import importlib.util
import io
import json
import os
import pathlib
import platform
import shutil
import subprocess
import tempfile
import tarfile
from unittest import mock


def verify_download_and_archive_guards():
    spec = importlib.util.spec_from_file_location("fixture_staging", pathlib.Path(__file__).with_name("prepare-gh-fixture-tools.py"))
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.valid_download_url("https://github.com/cli/cli/releases/download/x")
    for url in ("http://github.com/x", "https://attacker.invalid/x", "https://github.com@attacker.invalid/x", "https://github.com:444/x"):
        assert not module.valid_download_url(url), url
    try:
        module.FixtureRedirect().redirect_request(None, None, 302, "", {}, "https://attacker.invalid/x")
    except ValueError:
        pass
    else:
        raise AssertionError("untrusted redirect accepted")
    class Response:
        status = 200
        url = "https://github.com/tool"
        def __init__(self, payload): self.stream = io.BytesIO(payload)
        def __enter__(self): return self
        def __exit__(self, *_): return False
        def read(self, amount): return self.stream.read(amount)
    class Opener:
        def __init__(self, payload): self.payload = payload
        def open(self, *_args, **_kwargs): return Response(self.payload)
    with tempfile.TemporaryDirectory() as work:
        root = pathlib.Path(work)
        payload = b"synthetic-tool"
        expected = hashlib.sha256(payload).hexdigest()
        with mock.patch.object(module.ssl, "create_default_context", return_value=object()), \
             mock.patch.object(module.urllib.request, "HTTPSHandler", return_value=object()), \
             mock.patch.object(module.urllib.request, "build_opener", return_value=Opener(payload)):
            module.download("https://github.com/tool", root / "good", 100, expected)
            assert (root / "good").read_bytes() == payload
            for name, limit, digest in [("bad-digest", 100, "0" * 64), ("oversized", 2, expected)]:
                try:
                    module.download("https://github.com/tool", root / name, limit, digest)
                except ValueError:
                    pass
                else:
                    raise AssertionError(f"{name} accepted")
        def archive(path, members):
            with tarfile.open(path, "w:gz") as package:
                for name, body, kind in members:
                    info = tarfile.TarInfo(name)
                    info.type = kind
                    info.size = len(body) if kind == tarfile.REGTYPE else 0
                    if kind == tarfile.SYMTYPE: info.linkname = "target"
                    package.addfile(info, io.BytesIO(body) if info.size else None)
        good = root / "good.tar.gz"
        archive(good, [("release-plz", payload, tarfile.REGTYPE)])
        module.extract_binary(good, "release-plz", root / "tool", expected)
        assert (root / "tool").read_bytes() == payload
        for index, members in enumerate([
            [("release-plz", payload, tarfile.SYMTYPE)],
            [("release-plz", payload, tarfile.REGTYPE), ("release-plz", payload, tarfile.REGTYPE)],
            [("other", payload, tarfile.REGTYPE)],
        ]):
            case = root / f"case-{index}"
            case.mkdir()
            path = case / "bad.tar.gz"
            archive(path, members)
            try:
                module.extract_binary(path, "release-plz", case / "tool", expected)
            except ValueError:
                pass
            else:
                raise AssertionError("invalid archive accepted")
            assert not (case / "tool").exists()


def verify_git_staging_uses_bound_catalog():
    fixture_dir = pathlib.Path(__file__).resolve().parents[1] / "crates/velnor-actions-cli/tests/api2026_pull_context_fixture"
    spec = importlib.util.spec_from_file_location("fixture_driver", fixture_dir / "driver.py")
    assert spec and spec.loader
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    with tempfile.TemporaryDirectory() as work:
        root = pathlib.Path(work)
        source = root / "source"
        source.mkdir()
        tools = root / "tools"
        tools.mkdir()
        validation = root / "validation"
        catalog = validation / "crates/velnor-actions-mise/src/release_git_pin.rs"
        catalog.parent.mkdir(parents=True)
        values = {"pinned-git": b"synthetic-git", "pinned-git-remote-http": b"synthetic-remote"}
        for name, content in values.items(): (source / name).write_bytes(content)
        catalog.write_text('pub const GIT_SHA256: &str = "' + hashlib.sha256(values["pinned-git"]).hexdigest() + '";\n'
                           'pub const GIT_HTTP_SHA256: &str = "' + hashlib.sha256(values["pinned-git-remote-http"]).hexdigest() + '";\n')
        driver.stage_release_git(source, tools, validation)
        for name, content in values.items(): assert (tools / name).read_bytes() == content
        (source / "pinned-git").write_bytes(b"tampered")
        try:
            driver.stage_release_git(source, root / "unused", validation)
        except RuntimeError as error:
            assert "fixture tool does not match its qualified digest" in str(error)
        else:
            raise AssertionError("tampered Git executable accepted")


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
        gh = bindir / "gh"
        nextest = bindir / "cargo-nextest"
        gh.write_bytes(b"fixture-gh")
        nextest.write_bytes(b"fixture-nextest")
        gh.chmod(0o700)
        nextest.chmod(0o700)
        entry = {"tool_platform": tool_platform, "container_platform": container_platform,
                 "gh_sha256": hashlib.sha256(gh.read_bytes()).hexdigest(), "gh_archive_sha256": "0" * 64,
                 "nextest_sha256": hashlib.sha256(nextest.read_bytes()).hexdigest(), "nextest_archive_sha256": "0" * 64}
        manifest = {"gh_version": "2.103.3", "nextest_version": "0.9.151", "image": "rust@sha256:" + "a" * 64, arch_name: entry}
        (config / "tools.json").write_text(json.dumps(manifest))
        (config / "docker_host.py").write_text(
            "import os\ndef trusted_docker(): return os.environ['PATH'].split(':')[0] + '/docker'\n"
            "def docker_environment(): return {'PATH': os.environ['PATH']}\n"
            "def local_daemon(client, env): pass\n"
            "def verify_image(client, env, image): pass\n"
        )
        marker = root / "mise-executed"
        mise = bindir / "mise"
        mise.write_text(f'#!/bin/sh\nprintf executed > "{marker}"\nexit 0\n')
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
        assert not marker.exists()
        if arch_name == "x86_64":
            catalog = root / "crates/velnor-actions-mise/src/catalog.rs"
            catalog.parent.mkdir(parents=True)
            catalog.write_text('pub const MISE_BINARY_SHA256_LINUX_X64: &str = "' + "0" * 64 + '";')
            rejected = subprocess.run([*args, "--with-signed-controller"], env=env, capture_output=True)
            assert rejected.returncode != 0
            assert b"fixture tool digest mismatch" in rejected.stderr
            assert not marker.exists()
            assert not (dest / "mise_linux_amd64").exists()
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
    verify_download_and_archive_guards()
    verify_git_staging_uses_bound_catalog()
    main()
