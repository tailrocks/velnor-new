"""Bind the exact Cargo rlib closure used by the isolated rustc observer."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("closure_base", ROOT / "run.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def _stable_bytes(path):
    BASE.canonical(path)
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(fd)
        BASE.require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1,
                     "Cargo record must be an owned regular file")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read(64 * 1024 * 1024 + 1)
        BASE.require(len(data) <= 64 * 1024 * 1024, "Cargo output exceeds bound")
        after = os.fstat(fd)
        current = path.stat()
        BASE.require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                      before.st_ctime_ns) ==
                     (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                      after.st_ctime_ns) and len(data) == after.st_size and
                     (after.st_dev, after.st_ino) == (current.st_dev, current.st_ino) and
                     path == path.resolve(strict=True),
                     "Cargo output changed while being captured")
        return data, dict(path=str(path), size=len(data), sha256=hashlib.sha256(data).hexdigest())
    finally:
        os.close(fd)


def _cargo_messages(stdout, expected_receipt):
    data, receipt = _stable_bytes(stdout)
    BASE.require(receipt == expected_receipt,
                 "current Cargo build output differs from command receipt")
    messages = []
    finished = []
    finished_seen = False
    for line in data.splitlines(keepends=True):
        BASE.require(len(line) <= 8 * 1024 * 1024, "Cargo message exceeds bound")
        if not line.strip():
            continue
        value = json.loads(line)
        BASE.require(type(value) is dict, "Cargo JSON message must be an object")
        BASE.require(not finished_seen, "Cargo emitted messages after build-finished")
        if value.get("reason") == "compiler-artifact":
            messages.append(value)
        elif value.get("reason") == "build-finished":
            finished.append(value)
            finished_seen = True
    BASE.require(len(finished) == 1 and finished[0].get("success") is True,
                 "current Cargo build must finish successfully exactly once")
    return messages, receipt


def _metadata_index(metadata, workspace):
    packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    BASE.require(type(packages) is list and type(resolve) is dict,
                 "complete Cargo metadata resolution required")
    packages_by_id = {}
    for package in packages:
        BASE.require(type(package) is dict and isinstance(package.get("id"), str) and
                     package["id"] not in packages_by_id,
                     "Cargo metadata package identity is missing or duplicated")
        BASE.require(type(package.get("manifest_path")) is str and
                     Path(package["manifest_path"]).is_absolute(),
                     "Cargo metadata manifest path is not absolute")
        packages_by_id[package["id"]] = package
    root_candidates = [key for key, value in packages_by_id.items()
                       if value["manifest_path"] == str(workspace / "Cargo.toml")]
    BASE.require(len(root_candidates) == 1, "exactly one current fixture package required")
    root_id = root_candidates[0]
    nodes = resolve.get("nodes")
    BASE.require(type(nodes) is list, "Cargo metadata resolve nodes are missing")
    nodes_by_id = {}
    for node in nodes:
        BASE.require(type(node) is dict and node.get("id") in packages_by_id and
                     node["id"] not in nodes_by_id,
                     "Cargo resolution node is unknown or duplicated")
        nodes_by_id[node["id"]] = node
    BASE.require(root_id in nodes_by_id, "fixture package is absent from current resolution")
    return packages_by_id, nodes_by_id, root_id


def _target_key(target): return tuple((key, target.get(key)) for key in ("name", "kind", "crate_types", "src_path"))


def _package_targets(package, kind): return [target for target in package.get("targets", []) if kind in target.get("kind", [])]


def _active_runtime_dependencies(node):
    result = []
    deps = node.get("deps")
    BASE.require(type(deps) is list, "Cargo resolution dependency list is missing")
    for dependency in deps:
        BASE.require(type(dependency) is dict and isinstance(dependency.get("pkg"), str),
                     "malformed Cargo resolution dependency")
        kinds = dependency.get("dep_kinds")
        BASE.require(type(kinds) is list and kinds, "Cargo dependency kinds are missing")
        for item in kinds:
            BASE.require(type(item) is dict, "malformed Cargo dependency kind")
            kind = item.get("kind")
            BASE.require(kind in (None, "normal", "build", "dev"),
                         "unknown Cargo dependency kind")
            if kind in (None, "normal"):
                BASE.require(item.get("target") is None,
                             "target-specific normal dependency closure is unsupported")
                result.append(dependency["pkg"])
    return list(dict.fromkeys(result))


def _artifact_messages(stdout, packages_by_id, target_root, expected_receipt):
    index = {}
    messages, receipt = _cargo_messages(stdout, expected_receipt)
    for message in messages:
        package_id = message.get("package_id")
        BASE.require(package_id in packages_by_id,
                     "Cargo artifact is unbound to current metadata package")
        package = packages_by_id[package_id]
        BASE.require(message.get("manifest_path") == package.get("manifest_path"),
                     "Cargo artifact manifest differs from resolved package")
        target = message.get("target")
        BASE.require(type(target) is dict, "Cargo artifact target is missing")
        BASE.require(sum(_target_key(value) == _target_key(target)
                         for value in package.get("targets", [])) == 1,
                     "Cargo artifact target is unbound or ambiguous")
        key = package_id, target["name"]
        BASE.require(key not in index, "duplicate current Cargo compiler-artifact record")
        artifacts = []
        filenames = message.get("filenames")
        BASE.require(type(filenames) is list, "Cargo artifact filenames are missing")
        for name in filenames:
            path = Path(name)
            if path.suffix != ".rlib":
                continue
            BASE.canonical(path)
            BASE.require(path.is_relative_to(target_root),
                         "Cargo rlib escapes current owned target directory")
            info = path.stat()
            BASE.require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1,
                         "Cargo rlib must be an owned regular file")
            artifacts.append(BASE.artifact(path))
        index[key] = dict(package_id=package_id, manifest_path=message["manifest_path"],
                          target=target, artifacts=artifacts)
    return index, receipt


def public_rlib(stdout, workspace, target_root, metadata_path, metadata_receipt,
                expected_build_receipt):
    """Resolve only compiler artifacts emitted by this build and its fresh metadata."""
    BASE.canonical(workspace)
    BASE.canonical(target_root)
    BASE.require(target_root.is_relative_to(workspace.parent),
                 "Cargo target directory escapes the owned run")
    BASE.require(stdout == workspace.parent / "logs/library.stdout" and
                 metadata_path == workspace.parent / "logs/cargo-resolution.stdout",
                 "Cargo output must come from this owned run's current commands")
    metadata_bytes, current_metadata_receipt = _stable_bytes(metadata_path)
    BASE.require(current_metadata_receipt == metadata_receipt,
                 "Cargo metadata receipt changed before closure resolution")
    metadata = json.loads(metadata_bytes)
    BASE.require(type(metadata) is dict and metadata.get("version") == 1,
                 "unsupported Cargo metadata format")
    packages, nodes, root_id = _metadata_index(metadata, workspace)
    BASE.require(root_id in metadata.get("workspace_members", []),
                 "fixture package is not a current workspace member")
    root_package = packages[root_id]
    BASE.require(len(_package_targets(root_package, "lib")) == 1,
                 "exactly one public library target required")
    artifact_index, build_receipt = _artifact_messages(stdout, packages, target_root,
                                                        expected_build_receipt)

    visited = set()
    pending = [root_id]
    owners = []
    while pending:
        package_id = pending.pop(0)
        if package_id in visited:
            continue
        visited.add(package_id)
        package = packages[package_id]
        lib_targets = _package_targets(package, "lib")
        if package_id != root_id and not lib_targets:
            # Procedural macros execute during Cargo's build; their dependencies do not
            # participate in rustc's later link of the already-built public rlib.
            proc_targets = _package_targets(package, "proc-macro")
            BASE.require(proc_targets, "normal dependency has no linkable library target")
            continue
        BASE.require(len(lib_targets) == 1, "dependency must expose exactly one library target")
        entry = artifact_index.get((package_id, lib_targets[0]["name"]))
        BASE.require(entry is not None and len(entry["artifacts"]) == 1,
                     "current normal dependency rlib is missing or ambiguous")
        owners.append(dict(package_id=package_id, package_name=package["name"],
                           package_version=package["version"],
                           manifest_path=package["manifest_path"], target=entry["target"],
                           artifacts=entry["artifacts"]))
        for dependency_id in sorted(_active_runtime_dependencies(nodes[package_id])):
            BASE.require(dependency_id in packages and dependency_id in nodes,
                         "resolved normal dependency package is missing")
            if dependency_id not in visited:
                pending.append(dependency_id)

    BASE.require(owners and owners[0]["package_id"] == root_id,
                 "public rlib must lead the exact dependency closure")
    BASE.require(_stable_bytes(metadata_path)[1] == metadata_receipt and
                 _stable_bytes(stdout)[1] == build_receipt,
                 "Cargo resolution or build output changed during closure capture")
    return dict(artifact=owners[0]["artifacts"][0], package_id=root_id,
                workspace=str(workspace), target_root=str(target_root),
                crate_name=owners[0]["target"]["name"],
                resolution=metadata_receipt, build_stdout=build_receipt,
                public_artifact_owners=owners, native_authority=None)


def resolution_argv(cargo, workspace, build_argv):
    """Mirror only the supported locked/offline library build resolution scope."""
    BASE.require(cargo.is_absolute() and workspace.is_absolute(),
                 "pinned Cargo and workspace paths must be absolute")
    BASE.require(len(build_argv) >= 2 and build_argv[0] == "cargo" and
                 build_argv[1] == "build", "Cargo build command required for observer rlib")
    options = build_argv[2:]
    required = {"--locked", "--offline", "--lib", "--message-format=json-render-diagnostics"}
    BASE.require(len(options) == len(required) and set(options) == required,
                 "Cargo command has unsupported package, feature, target, or resolver options")
    return [cargo, "metadata", "--locked", "--offline", "--format-version", "1",
            "--manifest-path", workspace / "Cargo.toml"]


def validate_source_closure(closure):
    for owner in closure["public_artifact_owners"]:
        for expected in owner["artifacts"]:
            path = Path(expected["path"])
            BASE.canonical(path)
            BASE.require(path.is_relative_to(Path(closure["target_root"])) and
                         path.stat().st_nlink == 1 and BASE.artifact(path) == expected,
                         "Cargo dependency closure changed after it was captured")
    BASE.require(_stable_bytes(Path(closure["resolution"]["path"]))[1] == closure["resolution"],
                 "Cargo resolution receipt changed after it was captured")
    BASE.require(_stable_bytes(Path(closure["build_stdout"]["path"]))[1] == closure["build_stdout"],
                 "Cargo build message receipt changed after it was captured")


def stage_artifacts(closure, directory):
    """Copy only bound rlibs into a new isolated rustc search directory."""
    validate_source_closure(closure)
    BASE.require(directory.is_absolute() and not directory.exists() and not directory.is_symlink(),
                 "new isolated observer staging directory required")
    directory.mkdir(mode=0o700)
    BASE.require(stat.S_IMODE(directory.stat().st_mode) == 0o700,
                 "observer staging directory must be private")
    sources = [artifact for owner in closure["public_artifact_owners"]
               for artifact in owner["artifacts"]]
    names = [Path(item["path"]).name for item in sources]
    BASE.require(len(names) == len(set(names)), "dependency artifacts have colliding filenames")
    for source in sources:
        source_path = Path(source["path"])
        BASE.require(BASE.artifact(source_path) == source and source_path.stat().st_nlink == 1,
                     "bound Cargo artifact changed before staging")
        destination = directory / source_path.name
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        source_flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        source_fd = os.open(source_path, source_flags)
        try:
            dest_fd = os.open(destination, flags, 0o600)
            try:
                digest = hashlib.sha256()
                before = os.fstat(source_fd)
                BASE.require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1,
                             "Cargo artifact must be a single-link regular file")
                with os.fdopen(source_fd, "rb", closefd=False) as src, \
                     os.fdopen(dest_fd, "wb", closefd=False) as dst:
                    while True:
                        block = src.read(1024 * 1024)
                        if not block:
                            break
                        digest.update(block)
                        dst.write(block)
                    dst.flush()
                    os.fsync(dest_fd)
                after = os.fstat(source_fd)
                BASE.require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                              before.st_ctime_ns) ==
                             (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                              after.st_ctime_ns), "Cargo artifact changed while staging")
                BASE.require(digest.hexdigest() == source["sha256"] and
                             destination.stat().st_size == source["size"],
                             "staged Cargo artifact bytes differ from current compiler output")
                os.chmod(destination, 0o600)
            finally:
                os.close(dest_fd)
        finally:
            os.close(source_fd)
    expected = {Path(item["path"]).name: item for item in sources}
    inventory = staged_inventory(directory, expected)
    validate_source_closure(closure)
    return dict(directory=str(directory), inventory=inventory,
                source_artifacts=sources, native_authority=None)


def staged_inventory(directory, expected):
    BASE.canonical(directory)
    BASE.require(stat.S_IMODE(directory.stat().st_mode) == 0o700,
                 "observer staging directory permissions changed")
    paths = list(directory.iterdir())
    BASE.require({path.name for path in paths} == set(expected),
                 "staged dependency membership differs")
    inventory = []
    for path in sorted(paths):
        BASE.require(not path.is_symlink(), "staged dependency symlink forbidden")
        BASE.canonical(path)
        info = path.stat()
        BASE.require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                     stat.S_IMODE(info.st_mode) == 0o600,
                     "staged dependency must be a private regular file")
        actual = BASE.artifact(path)
        source = expected[path.name]
        BASE.require(actual["size"] == source["size"] and actual["sha256"] == source["sha256"],
                     "staged artifact differs from bound Cargo output")
        inventory.append(dict(**actual, mode="0o600"))
    return inventory
