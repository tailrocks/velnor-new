# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
import stat
from pathlib import Path

AUDITED = {
    "crates/velnor-actions-tofu/build.rs": "314b5af391a32cb3bebc52e0e1e2596b87e9c956f28397dd862240a63a0a8ae8",
    "crates/velnor-archive-guard/build.rs": "c087ccae097e3b26ba235b5804d4146e8e4b826a37e44a564e597135d6471052",
    "crates/velnor-archive-guard/build_support/archive_guard_inputs.rs": "ea9ef44a9b76cca194cc2a15d7908cab03b3743dde7b85d853e29abe90da001e",
    "crates/velnor-archive-guard/build_support/archive_guard_inputs_filesystem.rs": "f23c64b3c4d6b47946f1c3a960de23cd83ac10baa5474c591adb07c7b446c90a",
}
CUSTOM = {
    "velnor-actions-tofu": "crates/velnor-actions-tofu/build.rs",
    "velnor-archive-guard": "crates/velnor-archive-guard/build.rs",
}
MANIFEST = "scripts/archive-guard-inputs.txt"


def source_absent(root, path, native):
    current = root
    parts = path.split("/")
    for index, part in enumerate(parts):
        current = current / part
        try:
            info = current.lstat()
        except FileNotFoundError:
            return True
        if index == len(parts) - 1:
            return False
        if not stat.S_ISDIR(info.st_mode):
            raise native.InventoryError("archive guard absence has a non-directory ancestor")
    return False


def root_packages(metadata, root, native):
    if Path(metadata.get("workspace_root", "")).resolve() != root:
        raise native.InventoryError("root Cargo workspace mismatch")
    items = metadata.get("packages", [])
    nodes_list = (metadata.get("resolve") or {}).get("nodes", [])
    packages = {item["id"]: item for item in items}
    nodes = {item["id"]: item for item in nodes_list}
    if len(packages) != len(items) or len(nodes) != len(nodes_list):
        raise native.InventoryError("duplicate root Cargo identity")
    roots = [item["id"] for item in items if item["name"] == "velnor-actions-cli" and Path(item["manifest_path"]).resolve() == root / "crates/velnor-actions-cli/Cargo.toml"]
    if len(roots) != 1:
        raise native.InventoryError("root CLI package missing or ambiguous")
    seen = set()
    pending = roots[:]
    while pending:
        ident = pending.pop()
        if ident in seen:
            continue
        if ident not in packages or ident not in nodes:
            raise native.InventoryError("root Cargo dependency graph incomplete")
        seen.add(ident)
        for dep in nodes[ident].get("deps", []):
            kinds = dep.get("dep_kinds", [])
            if kinds and all(kind.get("kind") == "dev" for kind in kinds):
                continue
            pending.append(dep["pkg"])
    local = {}
    for ident in seen:
        package = packages[ident]
        if package.get("source") is not None:
            continue
        manifest = Path(package["manifest_path"]).resolve()
        if not manifest.is_relative_to(root):
            raise native.InventoryError("root local package escapes checkout")
        custom = [item for item in package.get("targets", []) if "custom-build" in item.get("kind", [])]
        if custom and (len(custom) != 1 or package["name"] not in CUSTOM or Path(custom[0]["src_path"]) != root / CUSTOM[package["name"]]):
            raise native.InventoryError("unsupported root Cargo build script")
        local[ident] = package
    if {package["name"] for package in local.values() if package["name"] in CUSTOM} != set(CUSTOM):
        raise native.InventoryError("audited root Cargo build scripts missing")
    if sum(package["name"] == "velnor-actions-contract" for package in local.values()) != 1:
        raise native.InventoryError("root contract package missing or ambiguous")
    return local


def manifest_inputs(root, target, canonical_target, tracked, native):
    result = set()
    absent = set()
    for path, expected in AUDITED.items():
        found = native.canonical_input(path, root, root, target, canonical_target, tracked)
        source = root / path
        if found != {path} or source.stat().st_size > 1_048_576 or hashlib.sha256(source.read_bytes()).hexdigest() != expected:
            raise native.InventoryError("audited root build script changed")
        result.update(found)
    manifest = root / MANIFEST
    if native.canonical_input(MANIFEST, root, root, target, canonical_target, tracked) != {MANIFEST}:
        raise native.InventoryError("archive guard manifest missing")
    result.add(MANIFEST)
    if manifest.stat().st_size > 32768:
        raise native.InventoryError("archive guard manifest too large")
    data = manifest.read_bytes()
    if len(data) > 32768 or not data.isascii() or not data.endswith(b"\n"):
        raise native.InventoryError("archive guard manifest format invalid")
    lines = data.decode("ascii").splitlines()
    if not lines or len(lines) > 512:
        raise native.InventoryError("archive guard manifest entry count invalid")
    seen = set()
    expanded = set()
    for line in lines:
        parts = line.split(" ")
        if len(parts) != 2 or parts[0] not in ("file", "optional", "tree"):
            raise native.InventoryError("archive guard manifest entry invalid")
        kind, path = parts
        if path in seen or not path or "\\" in path or Path(path).is_absolute() or any(part in ("", ".", "..") for part in path.split("/")):
            raise native.InventoryError("archive guard manifest path invalid")
        seen.add(path)
        source = root / path
        if kind == "optional" and source_absent(root, path, native):
            if path in tracked:
                raise native.InventoryError("tracked optional archive input missing")
            absent.add(source)
            continue
        if kind == "tree" and not source.is_dir():
            raise native.InventoryError("archive guard tree missing")
        if kind != "tree" and (not source.is_file() or source.is_symlink()):
            raise native.InventoryError("archive guard file invalid")
        found = native.canonical_input(path, root, root, target, canonical_target, tracked)
        if not found:
            raise native.InventoryError("archive guard input is not tracked")
        if expanded.intersection(found) or len(expanded) + len(found) > 512:
            raise native.InventoryError("archive guard inputs overlap or exceed the limit")
        expanded.update(found)
        result.update(found)
    return result, absent


def root_inputs(root, metadata, stream, target, canonical_target, tracked, native, private_file):
    packages = root_packages(metadata, root, native)
    if Path(os.path.normpath(metadata.get("target_directory", ""))) != target:
        raise native.InventoryError("root Cargo target mismatch")
    finished = [item for item in stream if item.get("reason") == "build-finished"]
    if len(finished) != 1 or finished[0].get("success") is not True or stream[-1] != finished[0]:
        raise native.InventoryError("root Cargo build did not finish successfully")
    messages = [item for item in stream if item.get("reason") == "compiler-artifact" and item.get("package_id") in packages]
    if set(packages) - {item.get("package_id") for item in messages}:
        raise native.InventoryError("root Cargo artifact stream incomplete")
    targets = [(item.get("target", {}).get("name"), tuple(item.get("target", {}).get("kind", []))) for item in messages]
    if targets.count(("velnor-actions", ("bin",))) != 1 or targets.count(("runner_family_inputs", ("example",))) != 1:
        raise native.InventoryError("root Cargo executables missing")
    for name in CUSTOM:
        ids = [ident for ident, package in packages.items() if package["name"] == name]
        builds = [item for item in stream if item.get("reason") == "build-script-executed" and item.get("package_id") in ids]
        if len(ids) != 1 or len(builds) != 1:
            raise native.InventoryError("root build-script output missing or ambiguous")
        out = Path(builds[0].get("out_dir", ""))
        if not out.is_absolute() or not out.is_relative_to(canonical_target / "debug") or out.is_symlink() or out.resolve(strict=True) != out:
            raise native.InventoryError("root build-script output outside private target")
    result, absent = manifest_inputs(root, target, canonical_target, tracked, native)
    debug = canonical_target / "debug"
    for package in packages.values():
        found = native.canonical_input(package["manifest_path"], root, root, target, canonical_target, tracked)
        if not found:
            raise native.InventoryError("root package manifest is not tracked")
        result.update(found)
    for item in messages:
        if item.get("fresh") is not False:
            raise native.InventoryError("root compiler reused an existing artifact")
        filenames = item.get("filenames", [])
        if not filenames or any(not private_file(name, debug) for name in filenames):
            raise native.InventoryError("root compiler artifact outside private debug target")
        dep = native.artifact_dep_info(item, root)
        if not private_file(dep, debug) or dep.stat().st_size > 16 * 1024 * 1024:
            raise native.InventoryError("root compiler dep-info outside private debug target")
        text = dep.read_text()
        for path in native.parse_make_prerequisites(text.split("\n\n", 1)[0], dep):
            found = native.canonical_input(path, root, root, target, canonical_target, tracked)
            absolute = Path(path) if Path(path).is_absolute() else root / path
            absolute = Path(os.path.normpath(absolute))
            if not found and absolute not in absent:
                if not absolute.is_relative_to(canonical_target) or not private_file(absolute, canonical_target):
                    raise native.InventoryError(f"root compiler input is unbound: {path}")
            result.update(found)
        package = packages[item["package_id"]]
        for line in text.splitlines():
            if not line.startswith("# env-dep:"):
                continue
            name, _, value = line.removeprefix("# env-dep:").partition("=")
            if name == "CARGO_PKG_VERSION" and value == package["version"]:
                continue
            if name == "CARGO_MANIFEST_DIR" and value == str(Path(package["manifest_path"]).parent):
                continue
            raise native.InventoryError("root compiler environment input is unbound")
    return result, {path.relative_to(root).as_posix() for path in absent}
