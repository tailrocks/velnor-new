"""Canonical source archive inventory; observation never mints publisher authority."""
from dataclasses import dataclass
import hashlib
import os

from source_archive_inventory_common import (
    MAX_BYTES, MAX_ENTRIES, MAX_MANIFEST, InventoryError, absolute, canonical,
    links, prefixes, recipe,
)
from source_archive_inventory_fs import root_descriptor
from source_archive_inventory_leaf import HardlinkInventory
from source_archive_inventory_walk import collect, root_entries, walk


@dataclass(frozen=True)
class InventoryResult:
    canonical_bytes: bytes
    digest: str
    files: int
    bytes: int


def _missing(path):
    return {"path": path, "kind": "missing", "mode": 0, "sha256": None, "target": None}


def _finish(entries, size, roots, optional, structural_prefixes, original_owner=None,
            owner_record=None):
    present = {entry["path"] for entry in entries}
    for root in roots:
        if root not in present:
            if root not in optional:
                raise InventoryError("payload_inventory_incomplete")
            entry = _missing(root)
            if original_owner is not None:
                entry.update(metadata=[], hardlink=None, local=None)
            entries.append(entry)
    entries.sort(key=lambda entry: entry["path"])
    if not entries:
        raise InventoryError("payload_inventory_incomplete")
    if len(entries) > MAX_ENTRIES or size > MAX_BYTES:
        raise InventoryError("payload_inventory_limit")
    links(entries, structural_prefixes, original_owner)
    record = {"schema": 3 if original_owner is not None else 2, "entries": entries}
    if original_owner is not None:
        if owner_record is None:
            raise InventoryError("source_original_owner_missing")
        record["owner"] = owner_record
    elif owner_record is not None:
        raise InventoryError("payload_owner_unqualified")
    data = canonical(record)
    if len(data) > MAX_MANIFEST:
        raise InventoryError("payload_manifest_limit")
    return InventoryResult(data, hashlib.sha256(data).hexdigest(),
                           sum(entry["kind"] == "file" for entry in entries), size)


def _inventory(root, roots, optional=(), exact=True):
    """Pure extraction seam; does not authenticate bytes or grant reuse."""
    absolute(root)
    admitted, missing = recipe(roots, optional)
    try:
        descriptor = root_descriptor(root)
        try:
            hardlinks = HardlinkInventory()
            entries, size = collect(walk(root, descriptor, hardlinks, roots=admitted, exact=exact))
            hardlinks.finish()
        finally:
            os.close(descriptor)
    except (OSError, RecursionError) as error:
        raise InventoryError("payload_inventory_unavailable") from error
    return _finish(entries, size, admitted, missing, prefixes(admitted) if exact else frozenset())


def _observe_numbered(quarantine, roots, optional=(), evidence_index=None):
    """Compiled-order physical observation; no target projection or receipt authority."""
    absolute(quarantine)
    admitted, missing = recipe(roots, optional)
    if evidence_index is not None and (type(evidence_index) is not int
                                       or evidence_index != len(admitted)):
        raise InventoryError("quarantine_evidence_index")
    entries, size = [], 0
    hardlinks = HardlinkInventory()
    try:
        boundary = root_descriptor(quarantine)
        try:
            descriptor = os.open("roots", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
                                 | os.O_CLOEXEC, dir_fd=boundary)
        finally:
            os.close(boundary)
        try:
            present = set(os.listdir(descriptor))
            allowed = {str(index) for index in range(len(admitted))}
            if evidence_index is not None:
                allowed.add(str(evidence_index))
            if not present.issubset(allowed):
                raise InventoryError("quarantine_unknown_index")
            for index, logical in enumerate(admitted):
                if str(index) not in present:
                    if logical not in missing:
                        raise InventoryError("quarantine_required_root_missing")
                    continue
                entries, size = collect(root_entries(quarantine, descriptor, index, logical,
                                                     hardlinks), entries, size)
            hardlinks.finish()
        finally:
            os.close(descriptor)
    except (OSError, RecursionError) as error:
        raise InventoryError("quarantine_inventory_unavailable") from error
    return entries, size, admitted, missing


def _inventory_numbered(quarantine, roots, optional=()):
    entries, size, admitted, missing = _observe_numbered(quarantine, roots, optional)
    return _finish(entries, size, admitted, missing, prefixes(admitted))


def source_archive_inventory(compiled_context, closed_capability):
    """No source factory is qualified; data and caller objects cannot activate it."""
    raise InventoryError("source_archive_projection_unqualified")
