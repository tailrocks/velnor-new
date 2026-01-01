#!/usr/bin/env python3
"""Build a source candidate; this command never claims behavioral qualification."""

import argparse
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import time

from owned_tool_source import (BUILD, HOSTS, PREFIX, admit, build_bootstrap_descriptor,
                               descriptor, digest, recipe, recipe_sha)


def environment(directory):
    # A subprocess allowlist removes GitHub/OIDC, cache, cloud and service secrets.
    result = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "COMSPEC",
              "SSL_CERT_FILE", "SSL_CERT_DIR", "LANG", "LC_ALL") if key in os.environ}
    result.update(HOME=str(directory / "home"), CARGO_HOME=str(directory / "cargo"),
                  RUSTUP_HOME=str(directory / "rustup"),
                  CARGO_TARGET_DIR=str(directory / "target"),
                  MISE_DATA_DIR=str(directory / "mise-data"),
                  MISE_CACHE_DIR=str(directory / "mise-cache"),
                  MISE_CONFIG_DIR=str(directory / "mise-config"),
                  XDG_CONFIG_HOME=str(directory / "config"),
                  XDG_CACHE_HOME=str(directory / "cache"),
                  TMPDIR=str(directory / "tmp"), CI="true", MISE_YES="1",
                  MISE_TRUSTED_CONFIG_PATHS="", MISE_PARANOID="1")
    for key in ("HOME", "CARGO_HOME", "RUSTUP_HOME", "TMPDIR"):
        Path(result[key]).mkdir()
    return result


def run(argv, source, env):
    started = time.monotonic_ns()
    try:
        result = subprocess.run(argv, cwd=source, env=env, check=False,
                                capture_output=True, text=True)
    except OSError as error:
        print(json.dumps({"command": argv, "returncode": None,
                          "duration_ns": time.monotonic_ns() - started,
                          "launch_error": str(error)}), flush=True)
        raise
    print(json.dumps({"command": argv, "returncode": result.returncode,
                      "duration_ns": time.monotonic_ns() - started}), flush=True)
    sys.stdout.write(result.stdout)
    sys.stdout.flush()
    sys.stderr.write(result.stderr)
    sys.stderr.flush()
    result.check_returncode()
    return result.stdout.strip()


def workflow_identity():
    result = {"commit": os.environ.get("GITHUB_SHA", ""),
              "run_id": os.environ.get("GITHUB_RUN_ID", ""),
              "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", "")}
    if not re.fullmatch(r"[a-f0-9]{40}", result["commit"]):
        raise ValueError("exact workflow commit required")
    if any(not re.fullmatch(r"[1-9][0-9]*", result[key]) for key in ("run_id", "run_attempt")):
        raise ValueError("hosted workflow run identity required")
    return result


def compiler_identity(text):
    lines = dict(line.split(": ", 1) for line in text.splitlines() if ": " in line)
    if lines.get("release") != "1.99.0" or lines.get("host") not in HOSTS:
        raise ValueError("unexpected effective compiler or native host")
    return lines["host"]


def binary_target(data):
    if (len(data) >= 64 and data[:7] == b"\x7fELF\x02\x01\x01"
            and int.from_bytes(data[16:18], "little") in (2, 3)):
        return {62: "x86_64-unknown-linux-gnu", 183: "aarch64-unknown-linux-gnu"}.get(
            int.from_bytes(data[18:20], "little"))
    if (len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe"
            and int.from_bytes(data[4:8], "little") == 0x0100000C
            and int.from_bytes(data[12:16], "little") == 2):
        return "aarch64-apple-darwin"
    return None


def package(binary, license_file, tool):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz", format=tarfile.USTAR_FORMAT) as archive:
        for name, file, mode in (("mise/bin/mise" if tool == "mise" else "mbx", binary, 0o755),
                                 ("mise/LICENSE" if tool == "mise" else "LICENSE", license_file, 0o644)):
            content = file if isinstance(file, bytes) else file.read_bytes()
            entry = tarfile.TarInfo(name)
            entry.mode, entry.size = mode, len(content)
            archive.addfile(entry, io.BytesIO(content))
    return buffer.getvalue()


def candidate(spec, bootstrap, directory, workflow):
    env = environment(directory)
    source = directory / "source"
    source_receipt = admit(spec, source, env)
    prefix = [str(bootstrap["mise"]), *PREFIX]
    compiler = run([*prefix, "rustc", "-vV"], source, env)
    host = compiler_identity(compiler)
    if host != os.environ.get("OWNED_TOOL_TARGET"):
        raise ValueError("effective compiler host differs from approved matrix target")
    linker = run([*prefix, "cc", "--version"], source, env)
    run([*prefix, str(bootstrap["mbx"]), *BUILD[spec["tool"]][1:]], source, env)
    binary = directory / "target/release" / spec["tool"]
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("build did not produce regular candidate executable")
    binary_data = binary.read_bytes()
    if binary_target(binary_data) != host:
        raise ValueError("candidate executable architecture differs from native host")
    banner = run([str(binary), "--version"], source, env)
    if binary.read_bytes() != binary_data:
        raise ValueError("candidate executable changed during version measurement")
    versions = re.findall(r"(?<![A-Za-z0-9.-])[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", banner)
    if versions != [spec["version"]] or "DEBUG" in banner:
        raise ValueError("candidate does not report exact owned release version")
    archive = package(binary_data, source / "LICENSE", spec["tool"])
    name = f'{spec["tool"]}-{spec["version"]}-{host}.tar.gz'
    workflow = {**workflow, "recipe_sha256": recipe_sha(spec["tool"])}
    receipt = {
        "schema": 1, "status": "SOURCE_BUILD_CANDIDATE", "tool": spec["tool"],
        "version": spec["version"], "target": host,
        "source": {"commit": spec["source_commit"], "tree": spec["source_tree"],
                   "archive_sha256": spec["archive_sha256"],
                   "receipt_sha256": spec["receipt_sha256"],
                   "lockfile_sha256": spec["lockfile_sha256"],
                   "base_patch_sha256": spec["patch_sha256"], "license_files": spec["license_files"]},
        "workflow": workflow,
        "artifact": {"name": name, "archive_sha256": digest(archive),
                     "binary_sha256": digest(binary_data)},
        "version_banner": banner, "compiler": {"rustc_vv": compiler, "linker": linker},
        "runner": {"image_os": os.environ.get("ImageOS", ""),
                   "image_version": os.environ.get("ImageVersion", "")},
        "recipe": recipe(spec["tool"]), "behavioral_qualification": None,
    }
    if not all(receipt["runner"].values()):
        raise ValueError("hosted runner image identity required")
    return name, archive, receipt, source_receipt


def build(output):
    workflow = workflow_identity()
    spec = descriptor(os.environ["OWNED_TOOL_SOURCE_JSON"], workflow["commit"])
    assets = build_bootstrap_descriptor(os.environ["OWNED_TOOL_BUILD_BOOTSTRAP_JSON"],
                                        os.environ["OWNED_TOOL_TARGET"])
    bootstrap = {}
    for tool, asset in assets.items():
        binary = Path(os.environ["VELNOR_BOOTSTRAP_" + tool.upper()])
        if (not binary.is_absolute() or binary.is_symlink() or not binary.is_file()
                or digest(binary.read_bytes()) != asset["binary_sha256"]
                or binary_target(binary.read_bytes()) != os.environ["OWNED_TOOL_TARGET"]):
            raise ValueError("verified absolute native bootstrap executable required")
        bootstrap[tool] = binary
    if output.exists() or output.is_symlink():
        raise ValueError("candidate destination already exists")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="owned-build-") as temporary:
        name, archive, receipt, source_receipt = candidate(spec, bootstrap, Path(temporary), workflow)
        output.mkdir(mode=0o700)
        (output / name).write_bytes(archive)
        (output / "source-receipt.json").write_bytes(source_receipt)
        (output / "candidate-receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        receipt = build(args.output.absolute())
    except (KeyError, TypeError, ValueError, OSError, tarfile.TarError,
            subprocess.CalledProcessError) as error:
        raise SystemExit("source candidate build failed: " + str(error)) from error
    print(json.dumps({"status": receipt["status"], "artifact": receipt["artifact"]}))


if __name__ == "__main__":
    main()
