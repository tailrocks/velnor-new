#!/usr/bin/env bash
set -euo pipefail

image_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly image_dir
repo_root="$(git -C "$image_dir" rev-parse --show-toplevel)"
readonly repo_root

python3 - "$repo_root" "$image_dir" <<'PY'
from pathlib import Path
import re
import sys
import tomllib

root = Path(sys.argv[1])
image = Path(sys.argv[2])
lock = tomllib.loads((root / ".velnor/runner.lock").read_text())
dockerfile = (image / "Dockerfile").read_text()
readme = (image / "README.md").read_text()
readme_text = re.sub(r"\s+", " ", readme)

base = f"{lock['base_image']}@{lock['base_image_digest']}"
if lock["base_image_platform"] != "linux/amd64":
    raise SystemExit(".velnor/runner.lock does not pin an amd64 base image.")
if dockerfile.count(f"FROM --platform=linux/amd64 {base}\n") != 1:
    raise SystemExit("Dockerfile must use the exact pinned amd64 Ubuntu image.")

docker_version = lock["docker_version"]
if docker_version != "29.9.1":
    raise SystemExit("The Docker version in .velnor/runner.lock changed. Examine the job image.")

artifacts = (
    ("Docker client", lock["docker_archive_url"], lock["docker_sha256"], "/tmp/docker.tgz"),
    (
        "Compose",
        lock["compose_asset_url"],
        lock["compose_sha256"],
        "/usr/local/lib/docker/cli-plugins/docker-compose",
    ),
    (
        "Buildx",
        lock["buildx_asset_url"],
        lock["buildx_sha256"],
        "/usr/local/lib/docker/cli-plugins/docker-buildx",
    ),
    ("Node.js", lock["node_archive_url"], lock["node_sha256"], "/tmp/node.tar.gz"),
)
for name, url, sha256, destination in artifacts:
    if not re.fullmatch(r"[0-9a-f]{64}", sha256):
        raise SystemExit(f"The {name} SHA-256 in .velnor/runner.lock has an incorrect format.")
    if url not in dockerfile or sha256 not in dockerfile:
        raise SystemExit(f"Dockerfile does not contain the pinned {name} URL or SHA-256.")
    digest_target = f"{sha256} \\\n        {destination}"
    if digest_target not in dockerfile:
        raise SystemExit(f"The {name} checksum has the wrong file path in Dockerfile.")
if dockerfile.count("sha256sum --check --status") != len(artifacts):
    raise SystemExit("Dockerfile must compare each artifact's SHA-256 with its pinned SHA-256.")

locked_checks = (
    f"[[ \"$(docker --version)\" == 'Docker version {docker_version}, build '* ]]",
    f"[[ \"$(docker compose version)\" == *'v{lock['compose_version']}'* ]]",
    f"[[ \"$(docker buildx version)\" == *'v{lock['buildx_version']}'* ]]",
    f"[[ \"$(node --version)\" == v{lock['node_version']} ]]",
    "tar -xzf /tmp/docker.tgz --strip-components=1 -C /usr/local/bin docker/docker",
    "tar -xzf /tmp/node.tar.gz --strip-components=1 -C /usr/local",
    "rm -f /tmp/docker.tgz",
    "rm -f /tmp/node.tar.gz",
    "[[ \"$(tar --version)\" == 'tar (GNU tar)'* ]]",
)
for expected in locked_checks:
    if expected not in dockerfile:
        raise SystemExit(f"Dockerfile does not contain a necessary tool command: {expected}")

packages = {
    "bash",
    "ca-certificates",
    "coreutils",
    "curl",
    "git",
    "gzip",
    "netcat-openbsd",
    "python3",
    "tar",
    "unzip",
    "xz-utils",
    "zstd",
}
install_section = dockerfile.split("apt-get install", 1)[1].split("rm -rf /var/lib/apt/lists/*", 1)[0]
missing = sorted(package for package in packages if not re.search(rf"(?m)^\s+{re.escape(package)}\s+\\?$", install_section))
if missing:
    raise SystemExit(f"Dockerfile does not contain necessary job packages: {', '.join(missing)}")

lower = dockerfile.lower()
for forbidden in (
    "actions-runner",
    "runner.listener",
    "runner-home",
    "jitconfig",
    "/velnor/",
    "resource-probe",
    "storage-init",
    "dockerd",
    "containerd",
    "node24",
):
    if forbidden in lower:
        raise SystemExit(f"The job Dockerfile contains Runner or controller data: {forbidden}")

if "Runner tools at `/__e`" not in readme_text:
    raise SystemExit("README must give the official Runner mount path /__e.")
if lock["runner_version"] != "2.338.0" or lock["runner_source_commit"] not in readme_text:
    raise SystemExit("README must give the pinned official Runner source.")
if "build-test.sh` compares the Dockerfile with `.velnor/runner.lock`" not in readme_text:
    raise SystemExit("README must give the source test procedure.")

print("PASS: Job image source agrees with .velnor/runner.lock.")
PY
