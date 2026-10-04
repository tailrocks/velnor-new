"""Faithful private baseline source derivation; native SDK owns lock authority.

Snapshots are observations of bytes, permission modes and directory layout.
They neither authenticate a registry nor construct a native tool capability.
"""
import os
import hashlib
import json
from pathlib import Path
import re
import stat
import tempfile


BASELINE_SOURCE_ID = "registry+https://github.com/rust-lang/crates.io-index"


def _baseline_path(path):
    raw = os.fspath(path)
    require(type(raw) is str and Path(raw).is_absolute() and str(Path(raw)) == raw and
            (raw == "/" or all(part not in ("", ".", "..") for part in raw.split("/")[1:]))
            and "\\" not in raw and "\x00" not in raw, "baseline_input_path")
    path = Path(raw)
    require(all(not item.is_symlink() for item in (path, *path.parents)),
            "baseline_input_symlink")
    return path


def _baseline_file(path):
    path = _baseline_path(path)
    mode = path.lstat().st_mode
    require(stat.S_ISREG(mode), "baseline_input_file_type")
    require(path.stat().st_size <= REGISTRY_FILE_LIMIT, "baseline_input_file_size")
    return stat.S_IMODE(mode), path.read_bytes()


def _baseline_walk_error(error):
    raise ReconcileError("baseline_source_walk") from error


def _baseline_tree(root):
    root = _baseline_path(root)
    mode = root.lstat().st_mode
    require(stat.S_ISDIR(mode), "baseline_source_root")
    observed = {"": (stat.S_IMODE(mode), None)}
    total = 0
    for directory, directories, files in os.walk(root, followlinks=False,
                                                onerror=_baseline_walk_error):
        for name in sorted([*directories, *files]):
            path = Path(directory) / name
            mode = path.lstat().st_mode
            require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), "baseline_source_type")
            raw = None
            if stat.S_ISREG(mode):
                require(path.stat().st_size <= REGISTRY_FILE_LIMIT,
                        "baseline_source_file_size")
                raw = path.read_bytes()
                total += len(raw)
            observed[path.relative_to(root).as_posix()] = (stat.S_IMODE(mode), raw)
            require(len(observed) <= REGISTRY_MEMBER_LIMIT and total <= REGISTRY_TAR_LIMIT,
                    "baseline_source_size")
    return observed


def _baseline_unchanged(trees, files):
    for root, snapshot in trees.items():
        require(_baseline_tree(root) == snapshot, "baseline_input_tree_changed")
    for path, snapshot in files.items():
        require(_baseline_file(path) == snapshot, "baseline_input_file_changed")


def _baseline_derived_unchanged(root, original, lock_relative, lock=None):
    observed = _baseline_tree(root)
    generated = observed.pop(lock_relative, None)
    require(observed == original, "baseline_derived_source_changed")
    if lock is not None:
        require(generated == lock, "baseline_derived_lock_changed")
    elif generated is not None:
        require(generated[1] is not None, "baseline_derived_lock_type")


def _baseline_registries(registries):
    require(type(registries) is list and registries, "baseline_registries")
    trees, sources = {}, {}
    for registry in registries:
        require(type(registry) is dict and set(registry) == {
                "source_id", "root", "index", "archives"}, "baseline_registry_fields")
        source = registry["source_id"]
        require(type(source) is str and source and source not in sources,
                "baseline_registry_source")
        require(type(registry["root"]) is str and Path(registry["root"]).is_absolute(),
                "baseline_registry_root")
        root = _baseline_path(registry["root"])
        trees[root] = _baseline_tree(root)
        for kind in ("index", "archives"):
            require(type(registry[kind]) is dict, "baseline_registry_inventory")
            for relative, digest in registry[kind].items():
                require(type(relative) is str and relative and
                        not Path(relative).is_absolute() and
                        all(part not in ("", ".", "..") for part in relative.split("/")) and
                        "\\" not in relative and "\x00" not in relative,
                        "baseline_registry_member")
                require(kind != "archives" or re.fullmatch(
                        r"[A-Za-z0-9][A-Za-z0-9_.+-]*\.crate", relative),
                        "baseline_registry_archive_basename")
                require(digest is None and kind == "index" or type(digest) is str and
                        len(digest) == 64 and all(c in "0123456789abcdef" for c in digest),
                        "baseline_registry_digest")
                path = _baseline_path(root / "index" / relative if kind == "index"
                                      else root / relative)
                if digest is None:
                    require(not path.exists(), "baseline_registry_negative_present")
                else:
                    require(hashlib.sha256(_baseline_file(path)[1]).hexdigest() == digest,
                            "baseline_registry_bytes")
        sources[source] = registry
    return trees, sources


def _baseline_observations(observations, sources, authenticated):
    require(type(observations) is list, "baseline_observations")
    seen = set()
    for observation in observations:
        require(type(observation) is dict and set(observation) == {
                "source_id", "kind", "path", "sha256"}, "baseline_observation_fields")
        source = observation["source_id"]
        kind = observation["kind"]
        require(type(source) is str and source in sources and kind in ("index", "archive"),
                "baseline_observation_source")
        inventory = sources[source]["index" if kind == "index" else "archives"]
        path = observation["path"]
        require(type(path) is str and path in inventory and
                observation["sha256"] == inventory[path], "baseline_observation_binding")
        key = (source, kind, path)
        require(key not in seen, "baseline_observation_duplicate")
        seen.add(key)
    needed = {(BASELINE_SOURCE_ID, "index", registry_index_path(authenticated["name"])),
              (BASELINE_SOURCE_ID, "archive",
               authenticated["name"] + "-" + authenticated["version"] + ".crate")}
    require(needed <= seen, "baseline_observation_missing_operation_read")


def _baseline_native_context(context, root, original=False):
    expected = {"workspace_manifest", "governing_lockfile", "governing_lockfile_relative"}
    require(type(context) is dict and set(context) == expected |
            ({"governing_lockfile_absent"} if original else set()), "baseline_native_context")
    if original:
        require(context["governing_lockfile_absent"] is True, "baseline_native_absence")
    relative = context["governing_lockfile_relative"]
    require(type(relative) is str and relative and not Path(relative).is_absolute() and
            all(part not in ("", ".", "..") for part in relative.split("/")) and
            "\\" not in relative and "\x00" not in relative and
            Path(relative).name == "Cargo.lock", "baseline_native_lock_relative")
    require(context["governing_lockfile"] == str(root / relative),
            "baseline_native_lock_scope")
    manifest = _baseline_path(context["workspace_manifest"])
    require(manifest.is_relative_to(root) and manifest.is_file() and
            manifest.name == "Cargo.toml", "baseline_native_workspace")
    return relative


def _baseline_native_result(value, work, authenticated, original, sources):
    require(type(value) is dict and set(value) == {"format", "result", "observations"} and
            type(value["format"]) is int and value["format"] == 1,
            "baseline_native_response")
    _baseline_observations(value["observations"], sources, authenticated)
    result = value["result"]
    require(type(result) is dict and set(result) == {"kind", "original_tree", "derived_tree",
            "original_context", "derived_context", "derived_lockfile",
            "derived_lockfile_sha256", "package_name", "package_version"} and
            result["kind"] == "derive_lock" and
            result["package_name"] == authenticated["name"] and
            result["package_version"] == authenticated["version"], "baseline_native_identity")
    derived = work / "source" / (authenticated["name"] + "-" + authenticated["version"])
    require(result["derived_tree"] == str(derived), "baseline_native_tree_scope")
    native_original = work / "original" / (authenticated["name"] + "-" + authenticated["version"])
    require(result["original_tree"] == str(native_original), "baseline_native_original_scope")
    require(_baseline_tree(native_original) == original, "baseline_native_original_source")
    relative = _baseline_native_context(result["original_context"], native_original, True)
    require(_baseline_native_context(result["derived_context"], derived) == relative,
            "baseline_native_context_relocation")
    require(Path(result["original_context"]["workspace_manifest"]).relative_to(native_original) ==
            Path(result["derived_context"]["workspace_manifest"]).relative_to(derived),
            "baseline_native_workspace_relocation")
    lock_path = derived / relative
    require(result["derived_lockfile"] == str(lock_path)
            and relative not in original, "baseline_existing_governing_lock")
    lock = _baseline_file(lock_path)
    require(lock[1] and hashlib.sha256(lock[1]).hexdigest() == result["derived_lockfile_sha256"],
            "baseline_native_lock_binding")
    _baseline_derived_unchanged(derived, original, relative, lock)
    return derived, relative, lock


def _baseline_tool_file(path):
    path = _baseline_path(path)
    mode = path.lstat().st_mode
    require(stat.S_ISREG(mode) and mode & 0o111, "baseline_compiler_pair_file")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return stat.S_IMODE(mode), digest.hexdigest()


def _baseline_compiler_pair(root):
    root = _baseline_path(root)
    binary = _baseline_path(root / "bin")
    require(root.is_dir() and binary.is_dir(), "baseline_compiler_pair_root")
    return root, {"root_mode": stat.S_IMODE(root.lstat().st_mode),
        "bin_mode": stat.S_IMODE(binary.lstat().st_mode),
        "cargo": _baseline_tool_file(binary / "cargo"),
        "rustc": _baseline_tool_file(binary / "rustc")}


def prepare_baseline_lock(authenticated, registries, output_root, resolve_native, sdk):
    """Use the sealed cold SDK compiler pair; native callback remains observation.

Literal loader supplies the canonical classes. This facade does not qualify a
generic callback as the source-owned native executable or execution boundary.
"""
    sdk_type = globals().get("ColdSourceIntentSdk")
    tool_type = globals().get("ColdSourceIntentInstalledTool")
    require(sdk_type is not None and tool_type is not None and type(sdk) is sdk_type,
            "baseline_sdk_authority")
    sdk.require_current()
    sdk.require_policy("1.98.1", "x86_64-unknown-linux-gnu")
    tools = {name: sdk.installed_tool(name) for name in ("cargo", "rustc")}
    for tool in tools.values():
        require(type(tool) is tool_type and tool._sdk is sdk, "baseline_sdk_tool_authority")
        tool.require_current()
    paths = {name: _baseline_path(tool.path) for name, tool in tools.items()}
    root, snapshot = _baseline_compiler_pair(paths["cargo"].parent.parent)
    for name in tools:
        require(paths[name] == root / "bin" / name and
                snapshot[name][1] == tools[name].sha256, "baseline_sdk_compiler_pair")
    try:
        return _prepare_baseline_lock_observed(authenticated, registries, output_root,
                                                resolve_native, root)
    finally:
        sdk.require_current()
        for tool in tools.values():
            tool.require_current()
        require(_baseline_compiler_pair(root) == (root, snapshot),
                "baseline_sdk_compiler_pair_changed")


def _prepare_baseline_lock_observed(authenticated, registries, output_root, resolve_native,
                                    toolchain_root):
    """Observe canonical native resolver; private SDK owns callback qualification.

The resolver consumes frozen local registries using a fresh Cargo home/cache,
offline config-free native Cargo without hooks. This source primitive grants no
authority from its request, response, callback or observed absence boolean.
"""
    toolchain_root, tool_snapshot = _baseline_compiler_pair(toolchain_root)
    authenticated = dict(authenticated)
    require(type(authenticated["name"]) is str and
            re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", authenticated["name"]) and
            type(authenticated["version"]) is str and
            re.fullmatch(r"[0-9A-Za-z.+-]+", authenticated["version"]), "baseline_package_path")
    original_root = _baseline_path(authenticated["package_root"])
    original = _baseline_tree(original_root)
    inventory = {path: {"sha256": hashlib.sha256(raw).hexdigest(),
                 "mode": "100755" if mode & 0o111 else "100644"}
                 for path, (mode, raw) in original.items() if raw is not None}
    encoded = json.dumps(inventory, sort_keys=True, separators=(",", ":")).encode()
    require(hashlib.sha256(encoded).hexdigest() == authenticated["inventory_sha256"],
            "baseline_original_binding")
    archive = _baseline_path(authenticated["archive_path"])
    archive_snapshot = _baseline_file(archive)
    require(hashlib.sha256(archive_snapshot[1]).hexdigest() ==
            authenticated["archive_sha256"] == authenticated["checksum"],
            "baseline_archive_binding")
    trees, sources = _baseline_registries(registries)
    require(BASELINE_SOURCE_ID in sources, "baseline_source_id")
    selected = sources[BASELINE_SOURCE_ID]
    selected_index = registry_index_path(authenticated["name"])
    selected_archive = authenticated["name"] + "-" + authenticated["version"] + ".crate"
    require(type(selected["index"].get(selected_index)) is str and
            selected["archives"].get(selected_archive) == authenticated["archive_sha256"],
            "baseline_registry_selected_inputs")
    trees[original_root] = original
    index = _baseline_path(authenticated["index_version_path"])
    files = {archive: archive_snapshot, index: _baseline_file(index)}
    require(hashlib.sha256(files[index][1]).hexdigest() ==
            authenticated["index_version_sha256"], "baseline_index_binding")
    output = _baseline_path(output_root)
    require(output.is_dir() and all(output != root and root not in output.parents
            for root in trees), "baseline_output_scope")
    container = Path(tempfile.mkdtemp(prefix="velnor-baseline-lock-", dir=output))
    work = container / "native"
    request = {"format": 1, "toolchain_root": str(toolchain_root),
        "registries": registries, "operation": {
        "kind": "derive_lock", "source_id": BASELINE_SOURCE_ID, "archive_path": str(archive),
        "archive_sha256": authenticated["archive_sha256"],
        "package_name": authenticated["name"], "package_version": authenticated["version"]},
        "work_root": str(work)}
    frozen_request = json.dumps(request, sort_keys=True, separators=(",", ":"), allow_nan=False)
    derived = work / "source" / (authenticated["name"] + "-" + authenticated["version"])
    native_original = work / "original" / (authenticated["name"] + "-" + authenticated["version"])
    relative, lock = None, None
    try:
        value = resolve_native(request)
        derived, relative, lock = _baseline_native_result(value, work, authenticated,
                                                          original, sources)
        trees[_baseline_path(value["result"]["original_tree"])] = original
        return value
    finally:
        require(_baseline_compiler_pair(toolchain_root) == (toolchain_root, tool_snapshot),
                "baseline_compiler_pair_changed")
        _baseline_unchanged(trees, files)
        require(_baseline_path(container).is_dir() and
                stat.S_IMODE(container.lstat().st_mode) == 0o700, "baseline_work_privacy_changed")
        if work.exists() or work.is_symlink():
            require(_baseline_path(work).is_dir() and
                    stat.S_IMODE(work.lstat().st_mode) == 0o700, "baseline_work_privacy_changed")
        require(json.dumps(request, sort_keys=True, separators=(",", ":"), allow_nan=False) ==
                frozen_request, "baseline_native_request_changed")
        if native_original.exists() or native_original.is_symlink():
            require(_baseline_tree(native_original) == original, "baseline_native_original_changed")
        if derived.exists() or derived.is_symlink():
            if relative is None:
                require(_baseline_tree(derived) == original, "baseline_unbound_derived_changed")
            else:
                _baseline_derived_unchanged(derived, original, relative, lock)
