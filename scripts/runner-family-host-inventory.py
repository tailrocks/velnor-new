#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path

HOST_BUILD = "crates/velnor-runner/crates/velnor-runner-host/build.rs"
HOST_BUILD_SHA256 = "698d48bc9909d422b51b47ce463afa43678c0fa79f9bab6388c8501397001571"
HOST_MANIFEST = "crates/velnor-runner/crates/velnor-runner-host/Cargo.toml"
HOST_IDENTITY = "crates/velnor-runner/crates/velnor-runner-host/src/compile_identity.rs"


def host_packages(metadata, root, native):
    workspace = root / "crates/velnor-runner"
    if Path(metadata.get("workspace_root", "")).resolve() != workspace:
        raise native.InventoryError("qualifier workspace root mismatch")
    items = metadata.get("packages", [])
    node_items = (metadata.get("resolve") or {}).get("nodes", [])
    packages = {item["id"]: item for item in items}
    nodes = {item["id"]: item for item in node_items}
    if len(packages) != len(items) or len(nodes) != len(node_items):
        raise native.InventoryError("duplicate qualifier Cargo identity")
    roots = [item["id"] for item in items if item["name"] == "velnor-runner-host" and Path(item["manifest_path"]).resolve() == root / HOST_MANIFEST]
    if len(roots) != 1:
        raise native.InventoryError("qualifier host package missing or ambiguous")
    pending = roots[:]
    seen = set()
    while pending:
        ident = pending.pop()
        if ident in seen:
            continue
        if ident not in packages or ident not in nodes:
            raise native.InventoryError("qualifier Cargo dependency graph incomplete")
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
            raise native.InventoryError("qualifier local package escapes checkout")
        custom = [item for item in package.get("targets", []) if "custom-build" in item.get("kind", [])]
        if (ident == roots[0] and (len(custom) != 1 or Path(custom[0]["src_path"]) != root / HOST_BUILD)) or (ident != roots[0] and custom):
            raise native.InventoryError("unsupported qualifier build script")
        local[ident] = package
    if roots[0] not in local:
        raise native.InventoryError("qualifier host package is not local")
    return local, roots[0]


def host_inputs(root, metadata, stream, target, canonical_target, tracked, native, private_file):
    packages, host = host_packages(metadata, root, native)
    if Path(os.path.normpath(metadata.get("target_directory", ""))) != target:
        raise native.InventoryError("qualifier Cargo target mismatch")
    finished = [item for item in stream if item.get("reason") == "build-finished"]
    if len(finished) != 1 or finished[0].get("success") is not True or not stream or stream[-1] != finished[0]:
        raise native.InventoryError("qualifier Cargo build did not finish successfully")
    messages = [item for item in stream if item.get("reason") == "compiler-artifact" and item.get("package_id") in packages]
    if set(packages) - {item.get("package_id") for item in messages}:
        raise native.InventoryError("qualifier compiler artifact stream incomplete")
    binaries = [item for item in messages if item.get("package_id") == host and item.get("target", {}).get("name") == "velnor-resource-probe-release" and item.get("target", {}).get("kind") == ["bin"]]
    if len(binaries) != 1 or binaries[0].get("executable") != str(canonical_target / "debug/velnor-resource-probe-release") or binaries[0].get("profile", {}).get("opt_level") != "0" or binaries[0].get("profile", {}).get("debug_assertions") is not True:
        raise native.InventoryError("qualifier binary missing or ambiguous")
    host_node = [node for node in metadata["resolve"]["nodes"] if node["id"] == host]
    if len(host_node) != 1 or binaries[0].get("features") != host_node[0].get("features"):
        raise native.InventoryError("qualifier binary features differ from metadata")
    build = root / HOST_BUILD
    if native.canonical_input(HOST_BUILD, root, root, target, canonical_target, tracked) != {HOST_BUILD} or build.stat().st_size > 1_048_576 or hashlib.sha256(build.read_bytes()).hexdigest() != HOST_BUILD_SHA256:
        raise native.InventoryError("audited qualifier build script changed")
    events = [item for item in stream if item.get("reason") == "build-script-executed" and item.get("package_id") == host]
    if len(events) != 1 or sorted(events[0].get("env", [])) != [["VELNOR_COMPILED_ATTESTATION_HELPER_SHA256", "unavailable"], ["VELNOR_COMPILED_SOURCE_SHA", "unavailable"]]:
        raise native.InventoryError("qualifier build identity is not unavailable")
    out = Path(events[0].get("out_dir", ""))
    debug = canonical_target / "debug"
    if not out.is_absolute() or not out.is_relative_to(debug) or out.is_symlink() or out.resolve(strict=True) != out:
        raise native.InventoryError("qualifier build output outside private target")
    generated = out / "compile_identity.rs"
    lock = out / "runner.lock"
    if not private_file(generated, debug) or generated.read_bytes() != b"const COMPILED_RELEASE_IDENTITY: Option<CompiledReleaseIdentity> = None;\n" or not private_file(lock, debug) or lock.read_bytes() != b"":
        raise native.InventoryError("qualifier generated identity mismatch")
    result = set()
    for path in (HOST_BUILD, HOST_MANIFEST, HOST_IDENTITY, ".velnor/runner.lock"):
        found = native.canonical_input(path, root, root, target, canonical_target, tracked)
        if found != {path}:
            raise native.InventoryError("qualifier required source missing")
        result.update(found)
    for package in packages.values():
        found = native.canonical_input(package["manifest_path"], root, root, target, canonical_target, tracked)
        if not found:
            raise native.InventoryError("qualifier package manifest is not tracked")
        result.update(found)
    source_env_seen = False
    out_env_seen = False
    for item in messages:
        if item.get("fresh") is not False:
            raise native.InventoryError("qualifier compiler reused an artifact")
        files = item.get("filenames", [])
        if not files or any(not private_file(path, debug) for path in files):
            raise native.InventoryError("qualifier artifact outside private target")
        dep = native.artifact_dep_info(item, root)
        if not private_file(dep, debug) or dep.stat().st_size > 16 * 1024 * 1024:
            raise native.InventoryError("qualifier dep-info outside private target")
        text = dep.read_text()
        for path in native.parse_make_prerequisites(text.split("\n\n", 1)[0], dep):
            found = native.canonical_input(path, root / "crates/velnor-runner", root, target, canonical_target, tracked)
            absolute = Path(path) if Path(path).is_absolute() else root / "crates/velnor-runner" / path
            absolute = Path(os.path.normpath(absolute))
            if not found and not private_file(absolute, debug):
                raise native.InventoryError(f"qualifier compiler input is unbound: {path}")
            result.update(found)
        for line in text.splitlines():
            if not line.startswith("# env-dep:"):
                continue
            value = line.removeprefix("# env-dep:")
            if packages[item["package_id"]]["name"] == "velnor-resource-probe" and value == "VELNOR_SOURCE_SHA":
                source_env_seen = True
            elif item["package_id"] == host and value == f"OUT_DIR={out}":
                out_env_seen = True
            else:
                raise native.InventoryError("qualifier compiler environment input is unbound")
    if not source_env_seen or not out_env_seen:
        raise native.InventoryError("qualifier compiler environment evidence incomplete")
    return result
