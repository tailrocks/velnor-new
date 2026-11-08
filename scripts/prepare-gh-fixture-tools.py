#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import ssl
import platform
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.parse
import urllib.request


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate manifest key")
        result[key] = value
    return result


def copy_verified(source, destination, expected, limit=128 * 1024 * 1024):
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= limit:
            raise ValueError("fixture tool is not a bounded regular file")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read(limit + 1)
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


def valid_download_url(url):
    parsed = urllib.parse.urlsplit(url)
    return (parsed.scheme == "https" and parsed.hostname in {
        "github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com",
        "static.rust-lang.org",
    } and parsed.username is None and parsed.password is None and parsed.port in (None, 443))


class FixtureRedirect(urllib.request.HTTPRedirectHandler):
    max_redirections = 5
    max_repeats = 2

    def __init__(self, allowed_host=None):
        super().__init__()
        self.allowed_host = allowed_host

    def redirect_request(self, request, response, code, message, headers, new_url):
        if (not valid_download_url(new_url)
                or (self.allowed_host is not None and urllib.parse.urlsplit(new_url).hostname != self.allowed_host)):
            raise ValueError("fixture download redirect is untrusted")
        return super().redirect_request(request, response, code, message, headers, new_url)


def download(url, destination, limit, expected, allowed_host=None):
    if (not valid_download_url(url)
            or (allowed_host is not None and urllib.parse.urlsplit(url).hostname != allowed_host)):
        raise ValueError("fixture download origin is untrusted")
    os.environ.pop("SSLKEYLOGFILE", None)
    context = ssl.create_default_context(cafile="/etc/ssl/certs/ca-certificates.crt")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), FixtureRedirect(allowed_host),
                                         urllib.request.HTTPSHandler(context=context))
    digest = hashlib.sha256()
    size = 0
    with opener.open(urllib.request.Request(url, headers={"User-Agent": "velnor-fixture"}), timeout=120) as response:
        if (response.status != 200 or not valid_download_url(response.url)
                or (allowed_host is not None and urllib.parse.urlsplit(response.url).hostname != allowed_host)):
            raise ValueError("fixture download response is invalid")
        with destination.open("xb") as output:
            while chunk := response.read(1024 * 1024):
                size += len(chunk)
                if size > limit:
                    raise ValueError("fixture download exceeds bound")
                digest.update(chunk)
                output.write(chunk)
    if not size or digest.hexdigest() != expected:
        raise ValueError("fixture download digest mismatch")


def extract_binary(archive, name, destination, digest):
    with tarfile.open(archive, "r:gz") as package:
        matches = [member for member in package if member.name == name]
        if len(matches) != 1 or not matches[0].isfile() or not 0 < matches[0].size <= 128 * 1024 * 1024:
            raise ValueError("fixture tool archive is invalid")
        binary = package.extractfile(matches[0])
        if binary is None:
            raise ValueError("fixture tool binary is missing")
        path = archive.parent / "extracted-tool"
        with path.open("xb") as output:
            shutil.copyfileobj(binary, output, length=1024 * 1024)
    copy_verified(path, destination, digest)


def stage_executable(command, destination, executable_digest, archive_url, archive_digest, member, env):
    candidate = shutil.which(command, path=env.get("PATH"))
    if candidate:
        try:
            copy_verified(pathlib.Path(candidate).resolve(strict=True), destination, executable_digest)
            return
        except (OSError, ValueError):
            pass
    with tempfile.TemporaryDirectory(dir=destination.parent, prefix=".download-") as directory:
        archive = pathlib.Path(directory) / "tool.tar.gz"
        download(archive_url, archive, 128 * 1024 * 1024, archive_digest)
        extract_binary(archive, member, destination, executable_digest)


def stage_source_compiler(root, destination, env):
    catalog = (root / "crates/velnor-actions-mise/src/catalog.rs").read_text()
    matches = re.findall(r'pub const MISE_BINARY_SHA256_LINUX_X64: &str\s*=\s*"([0-9a-f]{64})";', catalog)
    if len(matches) != 1:
        raise ValueError("fixture mise catalog pin is invalid")
    mise = shutil.which("mise", path=env.get("PATH"))
    if not mise:
        raise ValueError("fixture mise executable is missing")
    copy_verified(pathlib.Path(mise).resolve(strict=True), destination / "mise_linux_amd64", matches[0], 192 * 1024 * 1024)


def stage_signed_controller(root, destination, env):
    stage_source_compiler(root, destination, env)
    with tempfile.TemporaryDirectory(dir=destination, prefix=".signed-") as directory:
        root_dir = pathlib.Path(directory)
        channel = root_dir / "channel.toml"
        download("https://static.rust-lang.org/dist/channel-rust-1.99.1.toml", channel,
                 16 * 1024 * 1024, "fb152be80975c194aa5fc684b2aec3a6e14e2b855efe2faa4e5f8ae6e590f463")
        copy_verified(channel, destination / "channel-rust-1.99.1.toml",
                      "fb152be80975c194aa5fc684b2aec3a6e14e2b855efe2faa4e5f8ae6e590f463")
        archive = root_dir / "release-plz.tar.gz"
        download("https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-musl.tar.gz",
                 archive, 128 * 1024 * 1024, "4a4cfa92ea2abaee02b40daa596a95ca2725816a6a32cb03a8f48ddead287681")
        extract_binary(archive, "release-plz", destination / "release-plz-0.4.3",
                       "678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466")


def historical_rust_manifest_digest(root):
    catalog = (root / "crates/velnor-actions-contract/src/config/rust_channel_manifests.rs").read_text()
    matches = re.findall(r'\(\s*"1\.97\.1"\s*,\s*"([^"]*)"', catalog)
    if len(matches) != 1 or re.fullmatch(r'[0-9a-f]{64}', matches[0]) is None:
        raise ValueError("historical Rust manifest pin is invalid")
    return matches[0]


def stage_historical_rust_manifest(root, destination):
    digest = historical_rust_manifest_digest(root)
    output = destination / "channel-rust-1.97.1.toml"
    output.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(dir=destination, prefix=".historical-") as directory:
        manifest = pathlib.Path(directory) / "channel.toml"
        download("https://static.rust-lang.org/dist/channel-rust-1.97.1.toml", manifest,
                 16 * 1024 * 1024, digest)
        copy_verified(manifest, output, digest,
                      16 * 1024 * 1024)


def stage_historical_rust_components(root, destination):
    digest = historical_rust_manifest_digest(root)
    manifest_path = destination / "channel-rust-1.97.1.toml"
    fd = os.open(manifest_path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= 16 * 1024 * 1024:
            raise ValueError("historical Rust manifest is invalid")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            raw = stream.read(16 * 1024 * 1024 + 1)
        if len(raw) != metadata.st_size or hashlib.sha256(raw).hexdigest() != digest:
            raise ValueError("historical Rust manifest digest mismatch")
    finally:
        os.close(fd)
    manifest = tomllib.loads(raw.decode("utf-8"))
    target = "x86_64-unknown-linux-gnu"
    components = {}
    for name in ("cargo", "rustc", "rust-std"):
        item = manifest["pkg"][name]["target"][target]
        url = f"https://static.rust-lang.org/dist/2026-07-16/{name}-1.97.1-{target}.tar.xz"
        sha = item["xz_hash"]
        if (item.get("available") is not True or item["xz_url"] != url
                or not isinstance(sha, str) or re.fullmatch(r'[0-9a-f]{64}', sha) is None):
            raise ValueError("historical Rust component identity is invalid")
        components[name] = (url, sha)
    output = destination / "recovery-rust-components"
    if output.exists() or output.is_symlink():
        raise ValueError("recovery Rust component destination already exists")
    with tempfile.TemporaryDirectory(dir=destination, prefix=".recovery-rust-") as directory:
        stage = pathlib.Path(directory)
        for name, (url, sha) in components.items():
            download(url, stage / (name + ".tar.xz"), 256 * 1024 * 1024, sha,
                     allowed_host="static.rust-lang.org")
        try:
            output.mkdir(mode=0o700)
        except FileExistsError as error:
            raise ValueError("recovery Rust component destination already exists") from error
        try:
            os.rename(stage, output / "verified")
        except BaseException:
            shutil.rmtree(output)
            raise


RECOVERY_RELEASE_ASSETS = {
    "release-manifest.json": "8e71b846acd375086fcb9693e567985d5a48b902e2e4b84400724ca6daad32d9",
    "release-manifest.json.intoto.jsonl": "07acdc1fc01187e96d5545f7756cb769f3763ae2371f0a2785100545eac03f56",
    "velnor-actions-0.1.73-x86_64-unknown-linux-gnu": "e1b5224cedc462623de15056e75e76499245cb035bc70d20a2ca1a478bb68cd5",
    "velnor-actions-0.1.73-x86_64-unknown-linux-gnu.intoto.jsonl": "59292fc54dce584314f2664b6e9ac6d225f7f33c270c4688ed1f949a3f48ae4c",
}
RECOVERY_TRUSTED_ROOT_SHA256 = "65ca537f6ed8a47fd0e560c421baa1f6c1efb8b25fc200d8c5c02c0e92eb2b9c"


def stage_recovery_release_assets(root, destination, assets, trusted_root_digest):
    output = destination / "recovery-assets"
    if output.exists() or output.is_symlink():
        raise ValueError("recovery asset destination already exists")
    fixture = root / "crates/velnor-actions-orchestrator/tests/attestation_fixtures/historical/trusted-root.jsonl"
    with tempfile.TemporaryDirectory(dir=destination, prefix=".recovery-assets-") as directory:
        stage = pathlib.Path(directory)
        copy_verified(fixture, stage / "trusted-root.jsonl", trusted_root_digest, 4 * 1024 * 1024)
        for name, digest in assets.items():
            if (not isinstance(name, str) or name in (".", "..")
                    or re.fullmatch(r'[A-Za-z0-9._-]+', name) is None
                    or not isinstance(digest, str) or re.fullmatch(r'[0-9a-f]{64}', digest) is None):
                raise ValueError("recovery asset identity is invalid")
            download("https://github.com/tailrocks/velnor-new/releases/download/v0.1.73/" + name,
                     stage / name, 128 * 1024 * 1024, digest)
        try:
            output.mkdir(mode=0o700)
        except FileExistsError as error:
            raise ValueError("recovery asset destination already exists") from error
        try:
            os.rename(stage, output / "verified")
        except BaseException:
            shutil.rmtree(output)
            raise


def main():
    if sys.argv[1:] not in ([], ["--with-image-tuf"], ["--with-source-compiler"], ["--with-signed-controller"], ["--with-recovery-fixture"]):
        raise ValueError("unsupported fixture staging arguments")
    with_source_compiler = sys.argv[1:] == ["--with-source-compiler"]
    with_image_tuf = sys.argv[1:] == ["--with-image-tuf"]
    with_recovery_fixture = sys.argv[1:] == ["--with-recovery-fixture"]
    with_signed_controller = sys.argv[1:] == ["--with-signed-controller"] or with_recovery_fixture
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
    for key in ("gh_sha256", "gh_archive_sha256", "nextest_sha256", "nextest_archive_sha256"):
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
    gh_version = manifest["gh_version"]
    nextest_version = manifest["nextest_version"]
    if not all(isinstance(value, str) and value and all(c in "0123456789." for c in value) for value in (gh_version, nextest_version)):
        raise ValueError("invalid fixture tool version")
    staged_gh = destination / f"gh_{entry['tool_platform']}"
    stage_executable(
        "gh", staged_gh, entry["gh_sha256"],
        f"https://github.com/cli/cli/releases/download/v{gh_version}/gh_{gh_version}_{entry['tool_platform']}.tar.gz",
        entry["gh_archive_sha256"], f"gh_{gh_version}_{entry['tool_platform']}/bin/gh", env,
    )
    target = {"x86_64": "x86_64-unknown-linux-gnu", "aarch64": "aarch64-unknown-linux-gnu"}[arch]
    stage_executable(
        "cargo-nextest", destination / f"cargo-nextest_{entry['tool_platform']}", entry["nextest_sha256"],
        f"https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-{nextest_version}/cargo-nextest-{nextest_version}-{target}.tar.gz",
        entry["nextest_archive_sha256"], "cargo-nextest", env,
    )
    if with_source_compiler:
        if arch != "x86_64":
            raise ValueError("source compiler fixture requires Linux x86_64")
        stage_source_compiler(root, destination, env)
    if with_signed_controller:
        if arch != "x86_64":
            raise ValueError("signed controller fixture requires Linux x86_64")
        stage_signed_controller(root, destination, env)
        if with_recovery_fixture:
            stage_historical_rust_manifest(root, destination)
            stage_historical_rust_components(root, destination)
            stage_executable(
                "cargo-semver-checks", destination / "cargo-semver-checks-0.51.0",
                "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829",
                "https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v0.51.0/cargo-semver-checks-x86_64-unknown-linux-gnu.tar.gz",
                "cacadafaa5cd27ae6d05a74a103266917ebab4c5e43c898360e177d4fbf06a65",
                "cargo-semver-checks", env,
            )
            stage_recovery_release_assets(root, destination, RECOVERY_RELEASE_ASSETS,
                                          RECOVERY_TRUSTED_ROOT_SHA256)
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
