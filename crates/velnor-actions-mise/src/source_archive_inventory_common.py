"""Canonical source archive records and bounds; no receipt or caller authority."""
import json
import os

MAX_ENTRIES = 100000
MAX_BYTES = 16 * 1024 * 1024 * 1024
MAX_MANIFEST = 16 * 1024 * 1024
MAX_DEPTH = 128
MAX_PATH_BYTES = 4096
_ORIGINAL_OBSERVATION = object()


class InventoryError(ValueError):
    """A complete canonical observation could not be proved."""


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"),
                       ensure_ascii=True, allow_nan=False) + "\n").encode("ascii")


def relative(path):
    if (not isinstance(path, str) or not path or "\\" in path or "\0" in path
            or any(part in ("", ".", "..") for part in path.split("/"))):
        raise InventoryError("payload_path_invalid")
    if len(path.split("/")) > MAX_DEPTH or len(os.fsencode(path)) > MAX_PATH_BYTES:
        raise InventoryError("payload_path_limit")
    return path


def absolute(path):
    value = os.fspath(path)
    if not isinstance(value, str) or not value.startswith("/"):
        raise InventoryError("payload_absolute_root_required")
    components = value.split("/")[1:]
    if (not components or any(part in ("", ".", "..") for part in components)
            or "\\" in value or "\0" in value):
        raise InventoryError("payload_root_alias")
    return components


def stable(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_mode, metadata.st_nlink,
            metadata.st_size, metadata.st_uid, metadata.st_gid,
            metadata.st_mtime_ns, metadata.st_ctime_ns)


def recipe(roots, optional=()):
    admitted = tuple(relative(root) for root in roots)
    missing = tuple(relative(root) for root in optional)
    if not admitted or len(set(admitted)) != len(admitted):
        raise InventoryError("payload_roots_unqualified")
    if any(left != right and right.startswith(left + "/")
           for left in admitted for right in admitted):
        raise InventoryError("payload_roots_overlap")
    if len(set(missing)) != len(missing) or not set(missing).issubset(admitted):
        raise InventoryError("payload_optional_root_invalid")
    return admitted, missing


def prefixes(roots):
    result = set()
    for root in roots:
        components = root.split("/")
        result.update("/".join(components[:index]) for index in range(1, len(components)))
    return result


def numbered(path, roots):
    for index, root in enumerate(roots):
        if path == root or path.startswith(root + "/"):
            return "roots/" + str(index) + path[len(root):]
    raise InventoryError("payload_symlink_uncontained")


def _link_directory(path, by_path, structural_prefixes):
    if (path and path not in structural_prefixes
            and by_path.get(path, {}).get("kind") != "directory"):
        raise InventoryError("payload_symlink_ancestor")


def direct_target(path, target, by_path, structural_prefixes, original_owner=None):
    if (not target or "\\" in target or "\0" in target
            or len(os.fsencode(target)) > MAX_PATH_BYTES):
        raise InventoryError("payload_symlink_escape")
    if target.startswith("/"):
        if original_owner is None or not target.startswith(original_owner + "/"):
            raise InventoryError("payload_symlink_escape")
        stack, components = [], target[len(original_owner) + 1:].split("/")
    else:
        stack, components = path.split("/")[:-1], target.split("/")
    for index in range(1, len(stack) + 1):
        _link_directory("/".join(stack[:index]), by_path, structural_prefixes)
    for index, component in enumerate(components):
        if component == "..":
            if not stack:
                raise InventoryError("payload_symlink_escape")
            stack.pop()
        elif component not in ("", "."):
            stack.append(component)
        # Inspect raw traversal before a later '..' can erase a linked ancestor.
        if index < len(components) - 1:
            _link_directory("/".join(stack), by_path, structural_prefixes)
    resolved = "/".join(stack)
    if by_path.get(resolved, {}).get("kind") not in ("directory", "file"):
        raise InventoryError("payload_symlink_uncontained")
    return resolved


def links(entries, structural_prefixes=frozenset(), original_owner=None):
    by_path = {entry["path"]: entry for entry in entries}
    for entry in entries:
        if entry["kind"] == "symlink":
            direct_target(entry["path"], entry["target"], by_path,
                          structural_prefixes, original_owner)
