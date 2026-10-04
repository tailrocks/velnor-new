"""Locked SDK rustdoc production; caller owns executable/toolchain authority.

Executed in the release support shared namespace, which supplies ``require``.
``run(argv, cwd, environment_patch)`` must use the trusted constructor's fixed
SDK capability and approved environment. Context strings are data, never proof
that an executable or compiler was qualified. No CLI or ambient tool lookup.
"""
import base64
import binascii
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
from types import MappingProxyType


_DOC_CONTEXT_KEYS = frozenset({
    "cargo_executable", "rustdoc_toolchain", "target", "features",
    "use_default_features", "max_jobs", "output_root", "native_build_environment",
    "rustdoc_relative_directory", "generation_cwd"})


def _docs_context(root, context):
    require(type(context) is dict and set(context) == _DOC_CONTEXT_KEYS,
            "locked_docs_context")
    for key in ("cargo_executable", "rustdoc_toolchain", "output_root", "generation_cwd"):
        require(isinstance(context[key], str) and context[key] and
                "\x00" not in context[key], "locked_docs_context_string")
    cwd = Path(context["generation_cwd"])
    require(cwd.is_absolute() and cwd == cwd.resolve(strict=True) and cwd.is_dir(),
            "locked_docs_generation_cwd")
    require(context["target"] is None or isinstance(context["target"], str) and
            context["target"] and "\x00" not in context["target"],
            "locked_docs_target")
    build = context["native_build_environment"]
    require(type(build) is dict and set(build) == {"target_triple", "cargo_rustflags",
            "cargo_rustdocflags", "toolchain_version"} and
            all(isinstance(value, str) and "\x00" not in value for value in build.values()),
            "locked_docs_native_environment")
    require(build["target_triple"] and build["toolchain_version"] and
            context["rustdoc_toolchain"] == "rustdoc " + build["toolchain_version"],
            "locked_docs_toolchain_identity")
    relative = context["rustdoc_relative_directory"]
    require(isinstance(relative, str) and relative and "\\" not in relative and
            "\x00" not in relative and not Path(relative).is_absolute() and
            all(part not in ("", ".", "..") for part in relative.split("/")) and
            Path(relative).name == "doc", "locked_docs_relative_directory")
    features = context["features"]
    require(type(features) is list and all(isinstance(item, str) and item and
            not any(character == "," or character.isspace() or ord(character) < 32
                    or ord(character) == 127 for character in item) for item in features)
            and features == sorted(set(features)), "locked_docs_features")
    require(type(context["use_default_features"]) is bool, "locked_docs_defaults")
    cargo, output = _docs_execution_roots(root, context["cargo_executable"],
                                          context["output_root"], context["max_jobs"])
    require(output != cwd and cwd not in output.parents, "locked_docs_output_scope")
    return cargo, output


def _docs_execution_roots(root, cargo_executable, output_root, max_jobs):
    require(isinstance(cargo_executable, str) and "\x00" not in cargo_executable,
            "locked_docs_cargo_absolute")
    cargo = Path(cargo_executable)
    require(cargo.is_absolute(), "locked_docs_cargo_absolute")
    require(type(max_jobs) is int and 1 <= max_jobs <= 256,
            "locked_docs_jobs")
    require(isinstance(output_root, str) and "\x00" not in output_root,
            "locked_docs_output_absolute")
    output = Path(output_root)
    require(output.is_absolute(), "locked_docs_output_absolute")
    output = output.resolve(strict=True)
    require(output.is_dir() and output != root and root not in output.parents,
            "locked_docs_output_scope")
    return cargo, output


def _docs_inventory(root):
    """Include empty directories, untracked files, modes and link target bytes."""
    records = {}
    for directory, directories, files in os.walk(root, followlinks=False,
                                                onerror=_docs_walk_error):
        directory = Path(directory)
        if directory == root:
            directories[:] = [name for name in directories if name != ".git"]
            files = [name for name in files if name != ".git"]
        for path in [directory, *(directory / name for name in directories + files)]:
            relative = path.relative_to(root).as_posix()
            if relative in records:
                continue
            information = path.lstat()
            mode = information.st_mode
            require(stat.S_ISDIR(mode) or stat.S_ISREG(mode) or stat.S_ISLNK(mode),
                    "locked_docs_source_type")
            if stat.S_ISLNK(mode):
                target = path.resolve(strict=False)
                require(target == root or root in target.parents,
                        "locked_docs_source_link_scope")
                content = os.fsencode(os.readlink(path))
            else:
                require(not stat.S_ISREG(mode) or information.st_size <= 64 * 1024 * 1024,
                        "locked_docs_source_size")
                content = path.read_bytes() if stat.S_ISREG(mode) else b""
            require(len(content) <= 64 * 1024 * 1024, "locked_docs_source_size")
            records[relative] = (stat.S_IFMT(mode) | stat.S_IMODE(mode), content)
        directories[:] = [name for name in directories
                          if not (directory / name).is_symlink()]
    require(len(records) <= 100000, "locked_docs_source_count")
    return MappingProxyType(records)


def _docs_walk_error(error):
    raise error


def _docs_path(value, root, code, allow_root=False):
    require(isinstance(value, str) and Path(value).is_absolute(), code)
    path = Path(value)
    try:
        resolved = path.resolve(strict=True)
    except (OSError, RuntimeError):
        require(False, code)
    require(str(path) == value and path == resolved and
            (root in path.parents or allow_root and path == root), code)
    return path


def _docs_regular_snapshot(path, root, snapshot):
    relative = path.relative_to(root).as_posix()
    require(relative in snapshot and stat.S_ISREG(snapshot[relative][0]),
            "locked_docs_input_missing")
    return snapshot[relative][1]


def _docs_freeze_lock_observation(observation):
    """Validate an internal native observation; this never grants SDK authority."""
    keys = {"format", "workspaceManifest", "governingLockfile", "lockfileBytesBase64"}
    require(type(observation) in (dict, MappingProxyType) and set(observation) == keys,
            "locked_docs_governing_context")
    observation = dict(observation)
    require(type(observation["format"]) is int and observation["format"] == 1,
            "locked_docs_governing_format")
    require(all(type(observation[key]) is str and
                len(observation[key]) <= 32 * 1024 * 1024 for key in keys - {"format"}),
            "locked_docs_governing_fields")
    try:
        size = len(json.dumps(observation, ensure_ascii=False).encode("utf-8"))
    except UnicodeError:
        require(False, "locked_docs_governing_encoding")
    require(size <= 32 * 1024 * 1024, "locked_docs_governing_size")
    encoded = observation["lockfileBytesBase64"]
    require(len(encoded) <= ((16 * 1024 * 1024 + 2) // 3) * 4,
            "locked_docs_governing_bytes")
    try:
        raw = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError):
        require(False, "locked_docs_governing_bytes")
    require(0 < len(raw) <= 16 * 1024 * 1024 and
            base64.b64encode(raw).decode("ascii") == encoded,
            "locked_docs_governing_bytes")
    for key in ("workspaceManifest", "governingLockfile"):
        path = Path(observation[key])
        require("\x00" not in observation[key] and path.is_absolute() and
                str(path) == observation[key] and ".." not in path.parts,
                "locked_docs_governing_path")
    return MappingProxyType(observation), raw


def _docs_lock_context(root, observation, before):
    observation, raw = _docs_freeze_lock_observation(observation)
    workspace = _docs_path(observation["workspaceManifest"], root,
                           "locked_docs_governing_workspace")
    lock = _docs_path(observation["governingLockfile"], root,
                      "locked_docs_governing_lock")
    require(workspace.name == "Cargo.toml", "locked_docs_governing_workspace")
    _docs_regular_snapshot(workspace, root, before)
    require(_docs_regular_snapshot(lock, root, before) == raw,
            "locked_docs_governing_lock_bytes")
    return observation, workspace, lock, raw


def _docs_inputs_unchanged(root, before, lock_context):
    _, workspace, lock, raw = lock_context
    after = _docs_inventory(root)
    require(after == before, "locked_docs_source_changed")
    require(_docs_path(str(workspace), root, "locked_docs_governing_workspace") == workspace and
            _docs_path(str(lock), root, "locked_docs_governing_lock") == lock and
            _docs_regular_snapshot(lock, root, after) == raw,
            "locked_docs_governing_lock_bytes")


def _docs_run(run, argv, cwd, environment, root, before, lock_context):
    _docs_inputs_unchanged(root, before, lock_context)
    try:
        return run(argv, cwd, environment)
    finally:
        _docs_inputs_unchanged(root, before, lock_context)


def _docs_selection(raw, manifest, name, version, root, before, lock_context):
    require(isinstance(raw, bytes) and len(raw) <= 32 * 1024 * 1024,
            "locked_docs_metadata_size")
    metadata = json.loads(raw)
    require(type(metadata) is dict and type(metadata.get("packages")) is list and
            type(metadata.get("resolve")) is dict, "locked_docs_metadata_complete")
    matches = [package for package in metadata["packages"] if type(package) is dict
               and package.get("manifest_path") == str(manifest)
               and package.get("name") == name and package.get("version") == version]
    require(len(matches) == 1, "locked_docs_package_identity")
    package = matches[0]
    package_id = package.get("id")
    require(isinstance(package_id, str) and package_id and package.get("source") is None,
            "locked_docs_native_package")
    require(package_id in metadata.get("workspace_members", []),
            "locked_docs_workspace_member")
    nodes = metadata["resolve"].get("nodes")
    require(type(nodes) is list and sum(type(node) is dict and node.get("id") == package_id
                                      for node in nodes) == 1, "locked_docs_resolved_package")
    workspace = _docs_path(metadata.get("workspace_root"), root, "locked_docs_workspace", True)
    workspace_manifest = workspace / "Cargo.toml"
    _, captured_workspace, lock, _ = lock_context
    require(workspace_manifest == captured_workspace,
            "locked_docs_governing_workspace_metadata")
    inputs = {str(path): _docs_regular_snapshot(path, root, before)
              for path in (manifest, workspace_manifest, lock)}
    libraries = [target for target in package.get("targets", [])
                 if type(target) is dict and set(target.get("kind", [])) &
                 {"lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"}]
    require(len(libraries) == 1 and isinstance(libraries[0].get("name"), str) and
            libraries[0]["name"] and libraries[0]["name"] not in (".", "..") and
            not any(character in "/\\" or ord(character) < 32 or ord(character) == 127
                    for character in libraries[0]["name"]),
            "locked_docs_library")
    return metadata, package, libraries[0]["name"], workspace_manifest, lock, inputs


def _docs_features(context):
    result = []
    if context["features"]:
        result.extend(["--features", ",".join(context["features"])])
    if not context["use_default_features"]:
        result.append("--no-default-features")
    return result


def _docs_document(path, library, version):
    require(path.is_file() and not path.is_symlink(), "locked_docs_output_missing")
    require(path.stat().st_size <= 128 * 1024 * 1024, "locked_docs_output_size")
    raw = path.read_bytes()
    require(len(raw) <= 128 * 1024 * 1024, "locked_docs_output_size")
    document = json.loads(raw)
    require(type(document) is dict and type(document.get("format_version")) is int and
            document["format_version"] > 0, "locked_docs_format")
    if "crate_version" in document:
        require(document["crate_version"] == version, "locked_docs_crate_version")
    require(type(document.get("index")) is dict and "root" in document,
            "locked_docs_crate_root")
    crate = document["index"].get(str(document["root"]))
    require(type(crate) is dict and crate.get("name") == library,
            "locked_docs_crate_name")
    return raw, document["format_version"]


def _docs_execute(root, manifest, package_name, version, context, run, before, cargo, target,
                  lock_context):
    environment = {"CARGO_TARGET_DIR": str(target),
                   "CARGO_BUILD_JOBS": str(context["max_jobs"]), "CARGO_NET_OFFLINE": "true"}
    common = ["--locked", "--offline", "--manifest-path", str(manifest)]
    raw_metadata = _docs_run(run, [str(cargo), "metadata", *common, "--format-version", "1",
                                  *_docs_features(context)], Path(context["generation_cwd"]),
                             environment, root, before, lock_context)
    metadata, package, library, workspace_manifest, lock, inputs = _docs_selection(
        raw_metadata, manifest, package_name, version, root, before, lock_context)
    # Existing output is impossible: target is freshly allocated outside source.
    build = context["native_build_environment"]
    environment.update({"RUSTC_BOOTSTRAP": "1", "RUSTFLAGS": build["cargo_rustflags"],
                        "RUSTDOCFLAGS": build["cargo_rustdocflags"]})
    argv = [str(cargo), "rustdoc", *common, "--package", package_name, "--lib",
            "--target-dir", str(target), "--jobs", str(context["max_jobs"]),
            *_docs_features(context)]
    if context["target"] is not None:
        argv.extend(["--target", context["target"]])
    _docs_run(run, argv, Path(context["generation_cwd"]), environment, root, before, lock_context)
    path = target / context["rustdoc_relative_directory"] / (library + ".json")
    require(path.resolve(strict=True) == path, "locked_docs_output_link")
    rustdoc, format_version = _docs_document(path, library, version)
    return {"metadata_bytes": raw_metadata, "package_id": package["id"],
            "package_metadata": package, "rustdoc_bytes": rustdoc,
            "rustdoc_sha256": hashlib.sha256(rustdoc).hexdigest(),
            "format_version": format_version, "rustdoc_path": str(path),
            "manifest_path": str(manifest), "workspace_manifest_path": str(workspace_manifest),
            "lock_path": str(lock), "input_snapshots": MappingProxyType(inputs),
            "source_snapshot": before, "rustdoc_toolchain": context["rustdoc_toolchain"],
            "target": context["target"], "features": tuple(context["features"]),
            "native_build_environment": MappingProxyType(dict(build)),
            "rustdoc_relative_directory": context["rustdoc_relative_directory"],
            "generation_cwd": context["generation_cwd"],
            "governing_lock_context": lock_context[0],
            "use_default_features": context["use_default_features"],
            "resolved_features": tuple(next(node["features"] for node in metadata["resolve"]["nodes"]
                                             if node["id"] == package["id"]))}


def generate_locked_docs(root, manifest_path, package_name, version, context,
                         governing_lock_context, run):
    """Produce immutable inputs; the native checker consumes full metadata + ID."""
    root = Path(root).resolve(strict=True)
    require(root.is_dir(), "locked_docs_root")
    manifest = Path(manifest_path)
    if not manifest.is_absolute():
        manifest = root / manifest
    manifest = _docs_path(str(manifest), root, "locked_docs_manifest")
    require(manifest.name == "Cargo.toml" and isinstance(package_name, str) and package_name
            and isinstance(version, str) and version, "locked_docs_package")
    cargo, output = _docs_context(root, context)
    context = dict(context, features=list(context["features"]),
                   native_build_environment=dict(context["native_build_environment"]))
    before = _docs_inventory(root)
    _docs_regular_snapshot(manifest, root, before)
    lock_context = _docs_lock_context(root, governing_lock_context, before)
    target = Path(tempfile.mkdtemp(prefix="velnor-locked-docs-", dir=output))
    try:
        return _docs_execute(root, manifest, package_name, version, context, run,
                             before, cargo, target, lock_context)
    finally:
        _docs_inputs_unchanged(root, before, lock_context)


def read_locked_package(root, manifest_path, package_name, version, cargo_executable,
                        output_root, max_jobs, governing_lock_context, run):
    """Read full default metadata before the native owner chooses doc features."""
    root = Path(root).resolve(strict=True)
    require(root.is_dir(), "locked_docs_root")
    manifest = Path(manifest_path)
    if not manifest.is_absolute():
        manifest = root / manifest
    manifest = _docs_path(str(manifest), root, "locked_docs_manifest")
    require(manifest.name == "Cargo.toml" and isinstance(package_name, str) and package_name
            and isinstance(version, str) and version, "locked_docs_package")
    cargo, output = _docs_execution_roots(root, cargo_executable, output_root, max_jobs)
    before = _docs_inventory(root)
    _docs_regular_snapshot(manifest, root, before)
    lock_context = _docs_lock_context(root, governing_lock_context, before)
    target = Path(tempfile.mkdtemp(prefix="velnor-locked-metadata-", dir=output))
    environment = {"CARGO_TARGET_DIR": str(target), "CARGO_BUILD_JOBS": str(max_jobs),
                   "CARGO_NET_OFFLINE": "true"}
    try:
        raw = _docs_run(run, [str(cargo), "metadata", "--locked", "--offline", "--manifest-path",
                             str(manifest), "--format-version", "1"], root, environment,
                        root, before, lock_context)
        _, package, _, workspace_manifest, lock, inputs = _docs_selection(
            raw, manifest, package_name, version, root, before, lock_context)
        return {"metadata_bytes": raw, "package_id": package["id"],
                "package_metadata": package, "manifest_path": str(manifest),
                "workspace_manifest_path": str(workspace_manifest), "lock_path": str(lock),
                "input_snapshots": MappingProxyType(inputs), "source_snapshot": before,
                "governing_lock_context": lock_context[0]}
    finally:
        _docs_inputs_unchanged(root, before, lock_context)
