# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


GENERATOR = "scripts/native-image-candidate-inventory.py"
INVENTORY = "crates/velnor-actions-contract/src/image_candidate/closure_inventory.txt"
SUPPLEMENTAL = "crates/velnor-actions-contract/src/image_candidate/closure_supplemental.txt"
TARGET = "x86_64-unknown-linux-gnu"


class InventoryError(Exception):
    pass


def read_json(path):
    try:
        return json.loads(path.read_text())
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InventoryError(f"cannot read JSON input {path}") from error


def tracked_files(root):
    try:
        output = subprocess_run(["git", "-C", str(root), "ls-files", "-z"])
    except OSError as error:
        raise InventoryError("git ls-files failed") from error
    return {decode_path(item) for item in output.split(b"\0") if item}


def subprocess_run(args):
    completed = subprocess.run(args, check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if completed.returncode:
        raise InventoryError(completed.stderr.decode("utf-8", "replace").strip())
    return completed.stdout


def decode_path(value):
    try:
        return value.decode("utf-8")
    except UnicodeDecodeError as error:
        raise InventoryError("tracked source path is not UTF-8") from error


def make_prerequisites(path):
    try:
        text = path.read_text()
    except (OSError, UnicodeDecodeError) as error:
        raise InventoryError(f"cannot read compiler dep-info {path}") from error
    text = text.replace("\\\r\n", " ").replace("\\\n", " ")
    escaped = False
    separator = None
    for index, character in enumerate(text):
        if escaped:
            escaped = False
        elif character == "\\":
            escaped = True
        elif character == ":":
            separator = index
            break
    if separator is None:
        raise InventoryError(f"compiler dep-info has no rule separator: {path}")
    words = []
    word = []
    escaped = False
    for character in text[separator + 1 :]:
        if escaped:
            word.append(character)
            escaped = False
        elif character == "\\":
            escaped = True
        elif character.isspace():
            if word:
                words.append("".join(word))
                word.clear()
        else:
            word.append(character)
    if escaped:
        word.append("\\")
    if word:
        words.append("".join(word))
    return words


def workspace_packages(metadata, root):
    workspace_root = Path(metadata.get("workspace_root", "")).resolve()
    if workspace_root != root:
        raise InventoryError("Cargo metadata workspace root differs from the checkout")
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    nodes = {node["id"]: node for node in (metadata.get("resolve") or {}).get("nodes", [])}
    root_package = next(
        (
            package
            for package in packages.values()
            if package["name"] == "velnor-actions-cli"
            and Path(package["manifest_path"]).resolve()
            == root / "crates/velnor-actions-cli/Cargo.toml"
        ),
        None,
    )
    if root_package is None or root_package["id"] not in nodes:
        raise InventoryError("Cargo metadata does not resolve the candidate CLI package")
    reachable = set()
    pending = [root_package["id"]]
    while pending:
        package_id = pending.pop()
        if package_id in reachable:
            continue
        reachable.add(package_id)
        node = nodes.get(package_id)
        if node is None:
            raise InventoryError("Cargo metadata dependency graph is incomplete")
        for dependency in node.get("deps", []):
            kinds = dependency.get("dep_kinds", [])
            if kinds and all(kind.get("kind") == "dev" for kind in kinds):
                continue
            pending.append(dependency["pkg"])
    local = {}
    for package_id in reachable:
        package = packages.get(package_id)
        if package is None:
            raise InventoryError("Cargo metadata package table is incomplete")
        manifest = Path(package["manifest_path"]).resolve()
        if package.get("source") is None and manifest.is_relative_to(root):
            local[package_id] = package
    return local


def artifact_messages(path):
    try:
        lines = path.read_text().splitlines()
    except (OSError, UnicodeDecodeError) as error:
        raise InventoryError(f"cannot read Cargo artifact stream {path}") from error
    messages = []
    for line in lines:
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise InventoryError("Cargo artifact stream contains invalid JSON") from error
        if message.get("reason") in ("compiler-artifact", "build-script-executed"):
            messages.append(message)
    if not messages:
        raise InventoryError("Cargo artifact stream contains no compiler artifacts")
    return messages


def artifact_dep_info(message, root):
    target = message.get("target", {})
    filenames = [Path(name) for name in message.get("filenames", [])]
    filenames = [filename if filename.is_absolute() else root / filename for filename in filenames]
    if "custom-build" in target.get("kind", []):
        parents = {filename.parent for filename in filenames}
        matches = {
            parent / f"build_script_build-{parent.name.rsplit('-', 1)[-1]}.d"
            for parent in parents
        }
    else:
        matches = set()
        for filename in filenames:
            name = filename.name
            if name.startswith("lib"):
                name = name[3:]
            stem = Path(name).stem
            candidate = filename.parent / f"{stem}.d"
            matches.add(candidate if filename.suffix else filename.with_suffix(".d"))
    existing = sorted(path for path in matches if path.is_file())
    if len(existing) != 1:
        package = message.get("package_id", "unknown package")
        target_name = target.get("name", "unknown target")
        raise InventoryError(f"missing or ambiguous dep-info for {package} target {target_name}")
    return existing[0]


def canonical_input(raw_path, base, root, target_directory, canonical_target_directory, tracked):
    path = Path(raw_path)
    if not path.is_absolute():
        path = base / path
    path = Path(os.path.normpath(path))
    if not path.is_relative_to(root):
        return set()
    if path.is_relative_to(target_directory):
        resolved = path.resolve(strict=False)
        if resolved.is_relative_to(canonical_target_directory):
            return set()
        raise InventoryError(f"Cargo target input resolves outside its target directory: {raw_path}")
    relative_path = path.relative_to(root)
    relative = "" if relative_path == Path(".") else relative_path.as_posix()
    current = root
    for part in Path(relative).parts:
        current = current / part
        if current.is_symlink():
            raise InventoryError(f"compiler input traverses a symlink: {relative}")
    resolved = path.resolve(strict=False)
    if not resolved.is_relative_to(root):
        raise InventoryError(f"workspace input resolves outside the checkout: {raw_path}")
    if relative in tracked:
        if not path.is_file():
            raise InventoryError(f"tracked compiler input is not a regular file: {relative}")
        return {relative}
    if path.is_dir():
        reject_untracked_descendants(path, root, target_directory, canonical_target_directory, tracked)
        prefix = relative.rstrip("/") + "/" if relative else ""
        descendants = {item for item in tracked if item.startswith(prefix)}
        if not descendants:
            raise InventoryError(f"compiler input directory has no tracked files: {relative}")
        return descendants
    if path.exists():
        raise InventoryError(f"compiler input is not tracked: {relative}")
    return set()


def reject_untracked_descendants(path, root, target_directory, canonical_target_directory, tracked):
    for current, directories, files in os.walk(path, followlinks=False):
        current_path = Path(current)
        resolved = current_path.resolve(strict=False)
        if resolved.is_relative_to(canonical_target_directory):
            directories.clear()
            continue
        retained = []
        for name in directories:
            child = current_path / name
            if name == ".git":
                continue
            if child == target_directory:
                if child.resolve(strict=False) != canonical_target_directory:
                    raise InventoryError(f"Cargo target directory identity changed: {child}")
                continue
            retained.append(name)
        directories[:] = retained
        for name in directories:
            child = current_path / name
            if child.is_symlink():
                raise InventoryError(f"compiler input directory traverses a symlink: {child}")
        for name in files:
            child = current_path / name
            relative = child.relative_to(root).as_posix()
            if child.is_symlink():
                raise InventoryError(f"compiler input directory contains a symlink: {relative}")
            if relative not in tracked:
                raise InventoryError(f"compiler input directory contains untracked file: {relative}")


def build_inventory(root, metadata_path, artifacts_path, target):
    if target != TARGET:
        raise InventoryError("candidate inventory target differs from the release workflow target")
    root = root.resolve(strict=True)
    metadata = read_json(metadata_path)
    packages = workspace_packages(metadata, root)
    tracked = tracked_files(root)
    target_directory = Path(os.path.normpath(metadata["target_directory"]))
    if not target_directory.is_absolute():
        raise InventoryError("Cargo target directory is not absolute")
    canonical_target_directory = target_directory.resolve(strict=False)
    messages = artifact_messages(artifacts_path)
    emitted = {message.get("package_id") for message in messages}
    missing = sorted(set(packages) - emitted)
    if missing:
        raise InventoryError("Cargo artifact stream omits workspace packages: " + ", ".join(missing))
    inputs = set()
    for package in packages.values():
        manifest = Path(package["manifest_path"]).resolve()
        inputs.update(canonical_input(manifest, root, root, target_directory, canonical_target_directory, tracked))
    for relative in ("Cargo.lock", "Cargo.toml"):
        inputs.update(canonical_input(relative, root, root, target_directory, canonical_target_directory, tracked))
    supplemental_path = root / SUPPLEMENTAL
    try:
        supplemental = supplemental_path.read_text().splitlines()
    except (OSError, UnicodeDecodeError) as error:
        raise InventoryError(f"cannot read supplemental input list {SUPPLEMENTAL}") from error
    required = (GENERATOR, INVENTORY, SUPPLEMENTAL, *supplemental)
    missing_required = [relative for relative in required if relative not in tracked]
    if missing_required:
        raise InventoryError("required inventory inputs are not tracked: " + ", ".join(missing_required))
    for relative in required:
        inputs.update(canonical_input(relative, root, root, target_directory, canonical_target_directory, tracked))
    build_outputs = {}
    for message in messages:
        package_id = message.get("package_id")
        if message.get("reason") == "build-script-executed" and package_id in packages:
            if package_id in build_outputs or not isinstance(message.get("out_dir"), str):
                raise InventoryError("workspace build script has missing or ambiguous Cargo output")
            build_outputs[package_id] = Path(message["out_dir"]).parent / "output"
    matching = [message for message in messages if message.get("reason") == "compiler-artifact" and message.get("package_id") in packages]
    for message in matching:
        dep_info = artifact_dep_info(message, root)
        for dependency in make_prerequisites(dep_info):
            inputs.update(canonical_input(dependency, root, root, target_directory, canonical_target_directory, tracked))
        if "custom-build" in message.get("target", {}).get("kind", []):
            package_root = Path(packages[message["package_id"]]["manifest_path"]).resolve().parent
            output = build_outputs.get(message["package_id"])
            if output is None or not output.is_file():
                raise InventoryError(f"workspace build script has missing or ambiguous Cargo output: {message['package_id']}")
            try:
                lines = output.read_text().splitlines()
            except (OSError, UnicodeDecodeError) as error:
                raise InventoryError("cannot read workspace Cargo build-script output") from error
            rerun_paths = 0
            for line in lines:
                prefix = next(
                    (value for value in ("cargo:rerun-if-changed=", "cargo::rerun-if-changed=") if line.startswith(value)),
                    None,
                )
                if prefix is not None:
                    value = line[len(prefix) :]
                    if not value:
                        raise InventoryError("build script emitted an empty rerun-if-changed path")
                    inputs.update(canonical_input(value, package_root, root, target_directory, canonical_target_directory, tracked))
                    rerun_paths += 1
                if line.startswith(("cargo:rerun-if-env-changed=", "cargo::rerun-if-env-changed=")):
                    raise InventoryError("workspace build script has an unbound environment input")
                if line.startswith(("cargo:rustc-env=", "cargo::rustc-env=")):
                    _, assignment = line.split("=", 1)
                    name, separator, value = assignment.partition("=")
                    if name != "VELNOR_ARCHIVE_GUARD_FINGERPRINT" or not separator or len(value) != 64:
                        raise InventoryError("workspace build script has an unbound rustc environment input")
            if rerun_paths == 0:
                inputs.update(canonical_input(package_root, root, root, target_directory, canonical_target_directory, tracked))
    return "".join(f"{item}\n" for item in sorted(inputs)).encode()


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--target", required=True)
    arguments = parser.parse_args()
    root = arguments.root.resolve(strict=True)
    expected = build_inventory(root, arguments.metadata, arguments.artifacts, arguments.target)
    inventory_path = root / INVENTORY
    if arguments.write:
        inventory_path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=inventory_path.parent, delete=False) as temporary:
            temporary.write(expected)
            temporary_path = Path(temporary.name)
        os.replace(temporary_path, inventory_path)
        return 0
    try:
        actual = inventory_path.read_bytes()
    except OSError as error:
        raise InventoryError(f"cannot read checked-in inventory {INVENTORY}") from error
    if actual != expected:
        try:
            expected_set = set(expected.decode().splitlines())
            actual_set = set(actual.decode("utf-8").splitlines())
        except UnicodeDecodeError as error:
            raise InventoryError("checked-in source inventory is not UTF-8") from error
        missing = sorted(expected_set - actual_set)
        extra = sorted(actual_set - expected_set)
        if missing:
            print("native image source inventory is missing: " + ", ".join(missing[:20]), file=sys.stderr)
        if extra:
            print("native image source inventory has unexpected inputs: " + ", ".join(extra[:20]), file=sys.stderr)
        if not missing and not extra:
            print("native image source inventory order or newline format differs", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except InventoryError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
