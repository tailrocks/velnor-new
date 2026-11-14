#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("native_inventory", Path(__file__).with_name("native-image-candidate-inventory.py"))
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
root_spec = importlib.util.spec_from_file_location("runner_root_inventory", Path(__file__).with_name("runner-family-root-inventory.py"))
root_inventory = importlib.util.module_from_spec(root_spec)
root_spec.loader.exec_module(root_inventory)
host_spec = importlib.util.spec_from_file_location("runner_host_inventory", Path(__file__).with_name("runner-family-host-inventory.py"))
host_inventory = importlib.util.module_from_spec(host_spec)
host_spec.loader.exec_module(host_inventory)

INVENTORY = "crates/velnor-runner/crates/velnor-runner-image-manifest/src/runner_family_inventory.txt"
ABSENCES = "crates/velnor-runner/crates/velnor-runner-image-manifest/src/runner_family_absences.txt"
TARGET = "x86_64-unknown-linux-musl"
BINARIES = {"velnor-resource-probe", "velnor-storage-init", "velnor-job-start-hook"}


def private_input(path, limit):
    path = path.absolute()
    parent = path.parent
    if parent.resolve(strict=True) != parent:
        raise native.InventoryError("runner build input parent is not physical")
    parent_stat = parent.stat()
    if not stat.S_ISDIR(parent_stat.st_mode) or parent_stat.st_uid != os.geteuid() or parent_stat.st_mode & 0o077:
        raise native.InventoryError("runner build input parent is not private")
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open(path, flags)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_uid != os.geteuid() or before.st_nlink != 1 or before.st_mode & 0o077 or before.st_size > limit:
            raise native.InventoryError("runner build input is not a private regular file")
        chunks = []
        remaining = limit + 1
        while remaining:
            chunk = os.read(fd, min(remaining, 1024 * 1024))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        after = os.fstat(fd)
        if remaining == 0 or sum(map(len, chunks)) != before.st_size or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise native.InventoryError("runner build input changed or exceeded its limit")
        return b"".join(chunks)
    finally:
        os.close(fd)


def private_target_file(value, release_root):
    path = Path(value)
    if not path.is_absolute():
        return False
    try:
        relative = path.relative_to(release_root)
    except ValueError:
        return False
    current = release_root
    for part in relative.parts:
        if part in (".", ".."):
            return False
        current = current / part
        if current.is_symlink():
            return False
    if not path.is_file():
        return False
    info = path.stat()
    return info.st_uid == os.geteuid() and not info.st_mode & 0o022 and path.resolve(strict=True).is_relative_to(release_root)


def family_packages(metadata, root):
    workspace = root / "crates/velnor-runner"
    if Path(metadata.get("workspace_root", "")).resolve() != workspace:
        raise native.InventoryError("runner workspace root mismatch")
    package_items = metadata.get("packages", [])
    node_items = (metadata.get("resolve") or {}).get("nodes", [])
    packages = {item["id"]: item for item in package_items}
    nodes = {item["id"]: item for item in node_items}
    if len(packages) != len(package_items) or len(nodes) != len(node_items):
        raise native.InventoryError("duplicate runner Cargo metadata identity")
    wanted = [item for item in packages.values() if item["name"] == "velnor-resource-probe" and Path(item["manifest_path"]).resolve() == workspace / "crates/velnor-resource-probe/Cargo.toml"]
    if len(wanted) != 1:
        raise native.InventoryError("runner target package missing or ambiguous")
    pending = [wanted[0]["id"]]
    reachable = set()
    while pending:
        item = pending.pop()
        if item in reachable:
            continue
        if item not in packages:
            raise native.InventoryError("runner Cargo package metadata incomplete")
        reachable.add(item)
        node = nodes.get(item)
        if node is None:
            raise native.InventoryError("runner Cargo dependency graph incomplete")
        for dep in node.get("deps", []):
            kinds = dep.get("dep_kinds", [])
            if kinds and all(kind.get("kind") == "dev" for kind in kinds):
                continue
            pending.append(dep["pkg"])
    local = {}
    for key in reachable:
        package = packages[key]
        if package.get("source") is not None:
            continue
        if not Path(package["manifest_path"]).resolve().is_relative_to(root):
            raise native.InventoryError("local runner Cargo package outside checkout")
        if any("custom-build" in item.get("kind", []) for item in package.get("targets", [])):
            raise native.InventoryError("local runner Cargo build scripts are unsupported")
        local[key] = package
    if wanted[0]["id"] not in local:
        raise native.InventoryError("runner target package not local")
    return local, wanted[0]["id"]


def context_inputs(root, helper, target, canonical_target, tracked):
    inputs = set()
    absent = set()
    if not private_target_file(helper, canonical_target / "debug/examples"):
        raise native.InventoryError("runner context helper is outside private target")
    for role in ("runner", "dind", "resource_probe"):
        path_result = subprocess.run([str(helper), role, "--path"], check=True, capture_output=True, text=True, timeout=30)
        paths = path_result.stdout.splitlines()
        if len(paths) != 1:
            raise native.InventoryError("runner Dockerfile path invalid")
        dockerfile = paths[0]
        if native.canonical_input(dockerfile, root, root, target, canonical_target, tracked) != {dockerfile}:
            raise native.InventoryError("runner Dockerfile is not tracked")
        full = root / dockerfile
        for ignore in (full.parent / ".dockerignore", full.with_name(full.name + ".dockerignore")):
            if ignore.relative_to(root).as_posix() in tracked or os.path.lexists(ignore):
                raise native.InventoryError("runner Docker context ignore files are unsupported")
            absent.add(ignore.relative_to(root).as_posix())
        fd = os.open(full, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            before = os.fstat(fd)
            if not stat.S_ISREG(before.st_mode) or before.st_uid != os.geteuid() or before.st_nlink != 1 or before.st_size > 1_048_576:
                raise native.InventoryError("runner Dockerfile invalid")
            with os.fdopen(os.dup(fd), "rb") as source:
                content = source.read(1_048_577)
            after = os.fstat(fd)
            if len(content) != before.st_size or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                raise native.InventoryError("runner Dockerfile changed")
        finally:
            os.close(fd)
        result = subprocess.run([str(helper), role], input=content, check=True, capture_output=True, timeout=30)
        values = result.stdout.decode("utf-8").splitlines()
        if not values or values[0] != dockerfile or len(set(values)) != len(values):
            raise native.InventoryError("runner context helper output invalid")
        for value in values:
            if native.canonical_input(value, root, root, target, canonical_target, tracked) != {value}:
                raise native.InventoryError("runner Docker context input is not tracked")
            inputs.add(value)
    return inputs, absent


def inventory(root, metadata_path, artifacts_path, root_metadata_path, root_artifacts_path, host_metadata_path, host_artifacts_path, target, source_commit):
    if target != TARGET:
        raise native.InventoryError("runner family inventory target mismatch")
    if re.fullmatch(r"[0-9a-f]{40}", source_commit) is None:
        raise native.InventoryError("runner family source commit invalid")
    root = root.resolve(strict=True)
    if len({item.absolute().parent for item in (metadata_path, artifacts_path, root_metadata_path, root_artifacts_path, host_metadata_path, host_artifacts_path)}) != 1:
        raise native.InventoryError("runner build inputs do not share a private parent")
    try:
        metadata = json.loads(private_input(metadata_path, 32 * 1024 * 1024))
        stream = [json.loads(line) for line in private_input(artifacts_path, 32 * 1024 * 1024).splitlines()]
        root_metadata = json.loads(private_input(root_metadata_path, 32 * 1024 * 1024))
        root_stream = [json.loads(line) for line in private_input(root_artifacts_path, 32 * 1024 * 1024).splitlines()]
        host_metadata = json.loads(private_input(host_metadata_path, 32 * 1024 * 1024))
        host_stream = [json.loads(line) for line in private_input(host_artifacts_path, 32 * 1024 * 1024).splitlines()]
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise native.InventoryError("runner Cargo build input is invalid JSON") from error
    packages, package_id = family_packages(metadata, root)
    tracked = native.tracked_files(root)
    target_dir = Path(os.path.normpath(metadata["target_directory"]))
    if not target_dir.is_absolute():
        raise native.InventoryError("Cargo target directory is not absolute")
    if target_dir != metadata_path.absolute().parent / "target" or target_dir.is_symlink():
        raise native.InventoryError("runner Cargo target is not the private build target")
    canonical_target = target_dir.resolve(strict=True)
    target_stat = target_dir.stat()
    if canonical_target != target_dir or not stat.S_ISDIR(target_stat.st_mode) or target_stat.st_uid != os.geteuid() or target_stat.st_mode & 0o022:
        raise native.InventoryError("runner Cargo target ownership invalid")
    if (canonical_target / TARGET).is_symlink() or (canonical_target / TARGET / "release").is_symlink():
        raise native.InventoryError("runner Cargo release target traverses a symlink")
    finished = [item for item in stream if item.get("reason") == "build-finished"]
    if len(finished) != 1 or finished[0].get("success") is not True or stream[-1] != finished[0]:
        raise native.InventoryError("runner Cargo build did not finish successfully")
    messages = [item for item in stream if item.get("reason") in ("compiler-artifact", "build-script-executed")]
    if not messages:
        raise native.InventoryError("runner Cargo artifact stream contains no artifacts")
    emitted = {item.get("package_id") for item in messages}
    if set(packages) - emitted:
        raise native.InventoryError("runner Cargo artifact stream is incomplete")
    bins = [item for item in messages if item.get("reason") == "compiler-artifact" and item.get("package_id") == package_id and item.get("target", {}).get("name") in BINARIES and "bin" in item.get("target", {}).get("kind", [])]
    if len(bins) != len(BINARIES) or {item["target"]["name"] for item in bins} != BINARIES or any(item.get("profile", {}).get("opt_level") != "3" or item.get("profile", {}).get("debug_assertions") for item in bins):
        raise native.InventoryError("runner family release artifacts incomplete")
    env_inputs = set()
    inputs = set()
    for item in messages:
        if item.get("reason") != "compiler-artifact" or item.get("package_id") not in packages:
            continue
        if item.get("fresh") is not False:
            raise native.InventoryError("runner compiler reused an existing artifact")
        outputs = item.get("filenames", [])
        release_root = canonical_target / TARGET / "release"
        if not outputs or any(not private_target_file(output, release_root) for output in outputs):
            raise native.InventoryError("runner compiler artifact outside private release target")
        dep_info = native.artifact_dep_info(item, root)
        if not private_target_file(dep_info, release_root):
            raise native.InventoryError("runner compiler dep-info outside private release target")
        dep_text = dep_info.read_text()
        rule = dep_text.split("\n\n", 1)[0]
        for dependency in native.parse_make_prerequisites(rule, dep_info):
            found = native.canonical_input(dependency, root / "crates/velnor-runner", root, target_dir, canonical_target, tracked)
            if not found:
                raise native.InventoryError(f"runner compiler prerequisite is unbound: {dependency}")
            inputs.update(found)
        for line in dep_text.splitlines():
            if line.startswith("# env-dep:"):
                env_inputs.add(line.removeprefix("# env-dep:"))
    if env_inputs != {f"VELNOR_SOURCE_SHA={source_commit}"}:
        raise native.InventoryError("runner compiler environment inputs differ")
    root_paths, root_absent = root_inventory.root_inputs(root, root_metadata, root_stream, target_dir, canonical_target, tracked, native, private_target_file)
    host_target = Path(os.path.normpath(host_metadata["target_directory"]))
    if host_target != metadata_path.absolute().parent / "qualifier-target" or host_target.is_symlink():
        raise native.InventoryError("qualifier Cargo target is not private")
    canonical_host_target = host_target.resolve(strict=True)
    host_stat = host_target.stat()
    if canonical_host_target != host_target or not stat.S_ISDIR(host_stat.st_mode) or host_stat.st_uid != os.geteuid() or host_stat.st_mode & 0o022:
        raise native.InventoryError("qualifier Cargo target ownership invalid")
    host_paths = host_inventory.host_inputs(root, host_metadata, host_stream, host_target, canonical_host_target, tracked, native, private_target_file)
    context_paths, context_absent = context_inputs(root, canonical_target / "debug/examples/runner_family_inputs", target_dir, canonical_target, tracked)
    inputs.update(root_paths)
    inputs.update(host_paths)
    inputs.update(context_paths)
    absent = root_absent | context_absent
    required = ["Cargo.toml", "Cargo.lock", "crates/velnor-runner/Cargo.toml", "crates/velnor-runner/Cargo.lock", ".velnor/runner.lock", "mise.toml", "mise.lock", "rust-toolchain.toml", "scripts/check-runner-image-candidate.sh", "scripts/runner-family-source-inventory.py", "scripts/runner-family-root-inventory.py", "scripts/runner-family-host-inventory.py", "scripts/native-image-candidate-inventory.py", "scripts/generator-release/source-qualification.sh", "scripts/generator-release/cargo-git-handoff.py", "scripts/generator-release/cargo-source-handoff.sh", INVENTORY, ABSENCES]
    required.extend(str(Path(item["manifest_path"]).resolve()) for item in packages.values())
    for value in required:
        found = native.canonical_input(value, root, root, target_dir, canonical_target, tracked)
        if not found:
            raise native.InventoryError(f"required runner source input missing: {value}")
        inputs.update(found)
    if not absent or any(a == p or a.startswith(p + "/") or p.startswith(a + "/") for a in absent for p in inputs):
        raise native.InventoryError("runner source absence overlaps positive inputs")
    return ("".join(f"{item}\n" for item in sorted(inputs)).encode(),
            "".join(f"{item}\n" for item in sorted(absent)).encode())


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--root-metadata", type=Path, required=True)
    parser.add_argument("--root-artifacts", type=Path, required=True)
    parser.add_argument("--host-metadata", type=Path, required=True)
    parser.add_argument("--host-artifacts", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--source-commit", required=True)
    args = parser.parse_args()
    expected = inventory(args.root, args.metadata, args.artifacts, args.root_metadata, args.root_artifacts, args.host_metadata, args.host_artifacts, args.target, args.source_commit)
    paths = (args.root / INVENTORY, args.root / ABSENCES)
    if args.write:
        names = []
        for path, data in zip(paths, expected):
            with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
                output.write(data)
                names.append(output.name)
        for name, path in zip(names, paths):
            os.replace(name, path)
    elif any(path.read_bytes() != data for path, data in zip(paths, expected)):
        raise native.InventoryError("runner family source inventory differs from compiler inputs")


if __name__ == "__main__":
    try:
        main()
    except (native.InventoryError, OSError, KeyError, subprocess.SubprocessError, UnicodeError) as error:
        print(f"runner-family-source-inventory: {error}", file=sys.stderr)
        sys.exit(1)
