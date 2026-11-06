# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import json
import os
import pathlib
import platform
import re
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from docker_host import docker_environment, local_daemon, trusted_docker, verify_image
import source_snapshot

MANIFEST = json.loads(pathlib.Path(__file__).with_name("tools.json").read_text())
IMAGE = MANIFEST["image"]
MAX_FILE = 128 * 1024 * 1024
MAX_DEPENDENCY_TOTAL = 2 * 1024 * 1024 * 1024


def command(args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def read_fd(fd, max_size=MAX_FILE):
    before = os.fstat(fd)
    if not stat.S_ISREG(before.st_mode) or before.st_size > max_size:
        raise RuntimeError("fixture input is not a bounded regular file")
    with os.fdopen(fd, "rb", closefd=False) as f:
        data = f.read(max_size + 1)
    after = os.fstat(fd)
    if len(data) != before.st_size or (before.st_size, before.st_mtime_ns, before.st_ino, before.st_mode) != (after.st_size, after.st_mtime_ns, after.st_ino, after.st_mode):
        raise RuntimeError("fixture input changed while reading")
    return data, stat.S_IMODE(before.st_mode)


def read_regular(path, max_size=MAX_FILE):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        return read_fd(fd, max_size)[0]
    finally:
        os.close(fd)


def read_tracked(root, raw_name):
    parts = raw_name.split(b"/")
    if any(part in (b"", b".", b"..") for part in parts):
        raise RuntimeError("unsafe tracked fixture input")
    current = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=current)
            os.close(current)
            current = child
        fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW, dir_fd=current)
        try:
            return read_fd(fd)
        finally:
            os.close(fd)
    finally:
        os.close(current)


def checked_directory(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    meta = path.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise RuntimeError("fixture cache must be a private owned directory")


def registry_fingerprint(packages, registry):
    digest = hashlib.sha256()
    total = 0
    count = 0
    for package in sorted(packages):
        directory = registry / package
        if not directory.is_dir() or directory.is_symlink():
            raise RuntimeError("fixture dependency source is unavailable")
        def fail_walk(error):
            raise error
        for parent, dirs, files in os.walk(directory, followlinks=False, onerror=fail_walk):
            dirs.sort(); files.sort()
            for name in dirs:
                if (pathlib.Path(parent) / name).is_symlink():
                    raise RuntimeError("dependency symlink prevents complete fixture fingerprint")
            for name in files:
                relative = (pathlib.Path(parent) / name).relative_to(registry)
                raw_name = os.fsencode(relative)
                data, mode = read_tracked(registry, raw_name)
                count += 1; total += len(data)
                if count > 100000 or total > MAX_DEPENDENCY_TOTAL:
                    raise RuntimeError("fixture dependency sources exceed fingerprint bounds")
                digest.update(len(raw_name).to_bytes(8, "big")); digest.update(raw_name)
                digest.update(mode.to_bytes(4, "big")); digest.update(len(data).to_bytes(8, "big")); digest.update(data)
    return digest.digest()


def dependency_packages(metadata):
    packages = set()
    prefix = pathlib.PurePosixPath("/usr/local/cargo/registry")
    for package in metadata["packages"]:
        manifest = pathlib.PurePosixPath(package["manifest_path"])
        source = package["source"]
        if source is None:
            if not manifest.is_relative_to("/work"):
                raise RuntimeError("external path dependency prevents complete fixture fingerprint")
        elif source.startswith("registry+") and manifest.is_relative_to(prefix / "src"):
            packages.add(str(manifest.parent.relative_to(prefix)))
        else:
            raise RuntimeError("unsupported dependency source prevents complete fixture fingerprint")
    return packages


def fingerprint(entries, arch, profile, target_name, dependency_digest):
    digest = hashlib.sha256()
    digest.update(("velnor-gh-fixture-v2\0" + arch + "\0" + profile + "\0" + target_name + "\0" + IMAGE + "\0").encode())
    digest.update(dependency_digest)
    digest.update(source_snapshot.digest(entries))
    return digest.hexdigest()


def verify_sources(root, snapshot, entries, patterns=()):
    if source_snapshot.collect(root, None, read_tracked, patterns) != entries:
        raise RuntimeError("fixture tracked sources changed")
    source_snapshot.verify_snapshot(snapshot, entries, read_tracked)


def install_tool(source, target, expected):
    data = read_regular(source)
    if hashlib.sha256(data).hexdigest() != expected:
        raise RuntimeError("fixture tool does not match its qualified digest")
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        if hashlib.sha256(read_regular(target)).hexdigest() != expected:
            raise RuntimeError("fixture tool cache is corrupt")
        return
    fd, name = tempfile.mkstemp(dir=target.parent, prefix=".tool-")
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(data)
        os.chmod(name, 0o500)
        os.replace(name, target)
    finally:
        if os.path.exists(name): os.unlink(name)



def docker_base(client, root, target, tools, registry, image_platform):
    for path in [root, target, tools, registry]:
        if ":" in str(path) or "," in str(path):
            raise RuntimeError("fixture mount path is unsupported")
    return [client, "run", "--rm", "--pull=never", "--network=none", "--platform", image_platform,
            "--add-host", "api.github.com:127.0.0.1", "--cap-drop=ALL", "--cap-add=CHOWN", "--cap-add=SETUID", "--cap-add=SETGID", "--security-opt=no-new-privileges", "--mount", f"type=bind,src={root},dst=/work,readonly",
            "--mount", f"type=bind,src={target},dst=/target", "--mount", f"type=bind,src={tools},dst=/tools,readonly",
            "--mount", f"type=bind,src={registry},dst=/usr/local/cargo/registry,readonly", "-w", "/work", IMAGE]


def main():
    if len(sys.argv) != 4 or sys.argv[3] not in ("debug", "release"):
        raise RuntimeError("invalid isolated fixture invocation")
    kind, test, profile = sys.argv[1:]
    patterns = json.loads(os.environ["VELNOR_FIXTURE_NON_INPUTS"])
    if not isinstance(patterns, list) or len(patterns) > 1024:
        raise RuntimeError("invalid reviewed fixture exclusions")
    targets = {
        "cli": ("velnor-actions-cli", "api2026_pull_context_public"),
        "task_report": ("velnor-actions-orchestrator", "task_report_public"),
        "preview": ("velnor-actions-cli", "preview_source_selection_public"),
        "review": ("velnor-actions-cli", "review_completion_public"),
        "image_attestation": ("velnor-actions-orchestrator", None),
        "image_publication": ("velnor-actions-cli", "image_publication_public"),
    }
    if kind not in targets or (kind == "task_report" and test != "shared_job_artifact_is_downloaded_once") or (kind == "preview" and test != "public_preview_selection_uses_sha_media_and_preserves_parent_proofs") or (kind == "cli" and test in ("shared_job_artifact_is_downloaded_once", "public_preview_selection_uses_sha_media_and_preserves_parent_proofs")):
        raise RuntimeError("fixture scenario does not belong to target")
    package, test_target = targets[kind]
    image_attestation_tests = {
        "package_release_signer::attestation::image_transport_tests::real_github_transport_verifies_the_signed_record",
        "package_release_signer::attestation::image_fixture_tests::signed_image_record_binds_source_run_and_subjects",
    }
    if kind == "image_attestation" and test not in image_attestation_tests:
        raise RuntimeError("fixture scenario does not belong to target")
    if kind == "image_publication" and test not in (
        "attempt_identity::public_runner_publication_rejects_artifacts_from_other_or_ambiguous_attempts",
        "required_ci::public_publication_requires_one_successful_required_job_with_complete_identity",
    ):
        raise RuntimeError("fixture scenario does not belong to target")
    if not re.fullmatch(r"[a-z_]+(?:::[a-z_]+)*", test):
        raise RuntimeError("invalid isolated fixture scenario")
    machine = platform.machine().lower()
    arch = {"arm64":"aarch64", "aarch64":"aarch64", "x86_64":"x86_64", "amd64":"x86_64"}.get(machine)
    if arch is None:
        raise RuntimeError("unsupported isolated fixture host architecture")
    selected = MANIFEST[arch]
    image_platform, gh_platform = selected["container_platform"], selected["tool_platform"]
    gh_digest, nextest_digest = selected["gh_sha256"], selected["nextest_sha256"]
    expected_platform = "amd64" if arch == "x86_64" else "arm64"
    if (image_platform != f"linux/{expected_platform}"
            or gh_platform != f"linux_{expected_platform}"
            or not re.fullmatch(r"[0-9]{1,8}\.[0-9]{1,8}\.[0-9]{1,8}", MANIFEST["gh_version"])):
        raise RuntimeError("invalid fixture tool platform or version")
    client = trusted_docker()
    docker_env = docker_environment()
    local_daemon(client, docker_env)
    root = pathlib.Path(os.environ["VELNOR_FIXTURE_LIVE_ROOT"])
    validation = pathlib.Path(os.environ["VELNOR_FIXTURE_VALIDATION_ROOT"])
    manifest_path = pathlib.Path(os.environ["VELNOR_FIXTURE_CAPTURE_MANIFEST"])
    if not root.is_absolute() or not validation.is_absolute() or not manifest_path.is_absolute():
        raise RuntimeError("invalid captured fixture paths")
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise RuntimeError("duplicate captured fixture manifest key")
            result[key] = value
        return result
    manifest = json.loads(read_regular(manifest_path, 64 * 1024 * 1024), object_pairs_hook=unique_pairs)
    import raw_capture
    configured_tools = os.environ.get("VELNOR_GH_FIXTURE_TOOLS")
    if not configured_tools and os.environ.get("RUNNER_TEMP"):
        configured_tools = str(pathlib.Path(os.environ["RUNNER_TEMP"]) / "velnor-gh-fixture-tools")
    source_tools = pathlib.Path(configured_tools or "")
    if not source_tools.is_absolute():
        raise RuntimeError("set VELNOR_GH_FIXTURE_TOOLS to an absolute directory with qualified Linux gh and nextest")
    cargo_home = pathlib.Path(os.environ.get("CARGO_HOME") or (pathlib.Path.home() / ".cargo"))
    registry = (cargo_home / "registry").resolve(strict=True)
    if not registry.is_dir(): raise RuntimeError("offline Cargo registry is unavailable")
    cache = pathlib.Path(tempfile.gettempdir()) / f"velnor-gh-fixture-{os.getuid()}"
    checked_directory(cache)
    lock = os.open(cache / "build.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    snapshot_dir = tempfile.TemporaryDirectory(prefix="source-", dir=cache)
    snapshot = pathlib.Path(snapshot_dir.name)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        tools = cache / "tools"; checked_directory(tools)
        gh_version = MANIFEST["gh_version"]
        install_tool(source_tools / f"gh_{gh_platform}", tools / f"mise/installs/gh/{gh_version}/gh_{gh_version}_{gh_platform}/bin/gh", gh_digest)
        install_tool(source_tools / f"cargo-nextest_{gh_platform}", tools / "cargo-nextest", nextest_digest)
        verify_image(client, docker_env, IMAGE)
        entries = raw_capture.project(validation, snapshot, manifest, patterns)
        verify_sources(root, snapshot, entries, patterns)
        metadata_target = cache / "metadata"; checked_directory(metadata_target)
        metadata_docker = docker_base(client, snapshot, metadata_target, tools, registry, image_platform)
        triple = "x86_64-unknown-linux-gnu" if arch == "x86_64" else "aarch64-unknown-linux-gnu"
        metadata_script = f"export HOME=/tmp PATH=/usr/local/cargo/bin:/usr/bin:/bin RUSTUP_TOOLCHAIN=1.99.1-{triple} CARGO_TARGET_DIR=/target CARGO_NET_OFFLINE=true; exec cargo metadata --locked --offline --all-features --filter-platform {triple} --format-version 1"
        with tempfile.TemporaryFile() as output:
            command(metadata_docker[:-1] + ["--user", f"{os.getuid()}:{os.getgid()}", IMAGE, "sh", "-c", metadata_script], stdout=output, env=docker_env)
            output.seek(0)
            metadata_bytes = output.read(128 * 1024 * 1024 + 1)
        if len(metadata_bytes) > 128 * 1024 * 1024:
            raise RuntimeError("fixture dependency metadata exceeds its bound")
        packages = dependency_packages(json.loads(metadata_bytes))
        dependency_digest = registry_fingerprint(packages, registry)
        key = fingerprint(entries, arch, profile, kind, dependency_digest)
        target = cache / key; checked_directory(target)
        docker = docker_base(client, snapshot, target, tools, registry, image_platform)
        binary_meta = target / "fixture-binaries.json"; cargo_meta = target / "fixture-cargo.json"
        if not binary_meta.is_file() or not cargo_meta.is_file():
            release = "--release" if profile == "release" else ""
            selection = "--lib" if test_target is None else f"--test {test_target}"
            fixture_example = f"cargo build --locked --offline -p velnor-actions-cli --features generator --example image-publication-fixture {release};" if kind == "image_publication" else ""
            build = f'''set -eu; export HOME=/tmp PATH=/tools:/usr/local/cargo/bin:/usr/bin:/bin RUSTUP_TOOLCHAIN=1.99.1-{triple} CARGO_TARGET_DIR=/target CARGO_NET_OFFLINE=true; cargo metadata --locked --offline --all-features --filter-platform {triple} --format-version 1 > /target/fixture-cargo.tmp; {fixture_example} /tools/cargo-nextest nextest list --locked --offline -p {package} {selection} {release} --list-type binaries-only --message-format json > /target/fixture-binaries.tmp; mv /target/fixture-cargo.tmp /target/fixture-cargo.json; mv /target/fixture-binaries.tmp /target/fixture-binaries.json'''
            command(docker[:-1] + ["--user", f"{os.getuid()}:{os.getgid()}", IMAGE, "sh", "-c", build], env=docker_env)
            verify_sources(root, snapshot, entries, patterns)
            if fingerprint(entries, arch, profile, kind, registry_fingerprint(packages, registry)) != key:
                binary_meta.unlink(missing_ok=True); cargo_meta.unlink(missing_ok=True)
                raise RuntimeError("fixture sources changed during isolated build")
            (target / "fixture-key").write_text(key)
        verify_sources(root, snapshot, entries, patterns)
        if read_regular(target / "fixture-key", 64).decode() != key:
            raise RuntimeError("fixture cache identity mismatch")
        fcntl.flock(lock, fcntl.LOCK_UN)
        nonce = secrets.token_hex(32)
        receipt = target / f"receipt-{nonce}"
        script = f'''set -eu; mkdir /tmp/fixture-tls /tmp/fixture-home; openssl req -x509 -newkey rsa:2048 -nodes -keyout /tmp/fixture-tls/key.pem -out /tmp/fixture-tls/cert.pem -days 1 -subj /CN=api.github.com -addext subjectAltName=DNS:api.github.com >/dev/null 2>&1; cat /tmp/fixture-tls/cert.pem >> /etc/ssl/certs/ca-certificates.crt; chown -R {os.getuid()}:{os.getgid()} /tmp/fixture-tls /tmp/fixture-home; exec setpriv --reuid={os.getuid()} --regid={os.getgid()} --clear-groups /bin/sh -c 'export HOME=/tmp/fixture-home PATH=/tools:/usr/local/cargo/bin:/usr/bin:/bin MISE_DATA_DIR=/tools/mise VELNOR_GH_TEST_TLS_ROOT=/tmp/fixture-tls VELNOR_GH_FIXTURE_WORKER=container-v1 VELNOR_GH_FIXTURE_SCENARIO="$FIXTURE_SCENARIO" VELNOR_GH_FIXTURE_TARGET="$FIXTURE_TARGET" VELNOR_GH_FIXTURE_CACHE_KEY="$FIXTURE_KEY" VELNOR_GH_FIXTURE_ARCH="$FIXTURE_ARCH" VELNOR_GH_FIXTURE_NONCE="$FIXTURE_NONCE"; if [ "$FIXTURE_TARGET" = image_attestation ]; then export GH_TOKEN=fixture_token VELNOR_TEST_ATTESTATION_ROOT=/work/crates/velnor-actions-orchestrator/tests/attestation_fixtures/historical/trusted-root.jsonl; fi; if [ "$FIXTURE_TARGET" = image_publication ]; then export VELNOR_IMAGE_PUBLICATION_FIXTURE="/target/$FIXTURE_PROFILE/examples/image-publication-fixture"; fi; exec /tools/cargo-nextest nextest run --cargo-metadata /target/fixture-cargo.json --binaries-metadata /target/fixture-binaries.json -E "test(=$FIXTURE_SCENARIO)" --no-tests fail' '''
        env_args = ["-e", f"FIXTURE_SCENARIO={test}", "-e", f"FIXTURE_TARGET={kind}", "-e", f"FIXTURE_KEY={key}", "-e", f"FIXTURE_ARCH={arch}", "-e", f"FIXTURE_NONCE={nonce}", "-e", f"FIXTURE_PROFILE={profile}"]
        try:
            command(docker[:-1] + env_args + [IMAGE, "sh", "-c", script], env=docker_env)
            if read_regular(receipt, 256).decode() != f"{nonce}\n{kind}\n{test}\n":
                raise RuntimeError("fixture worker success receipt does not match scenario")
        finally:
            receipt.unlink(missing_ok=True)
        verify_sources(root, snapshot, entries, patterns)
        if fingerprint(entries, arch, profile, kind, registry_fingerprint(packages, registry)) != key:
            raise RuntimeError("fixture sources changed during isolated execution")
    finally:
        snapshot_dir.cleanup()
        os.close(lock)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        print(f"isolated GitHub fixture: {error}", file=sys.stderr)
        sys.exit(1)
