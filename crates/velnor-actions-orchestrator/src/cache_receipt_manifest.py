"""Receipt errors around the sole canonical source archive inventory."""
import os

from cache_receipt_common import ColdReceipt
from source_archive_inventory import _inventory
from source_archive_inventory_common import MAX_BYTES, MAX_ENTRIES, MAX_MANIFEST
import source_archive_inventory_common as records
import source_archive_inventory_fs as filesystem
import source_archive_inventory_leaf as leaves


def _receipt(function, *arguments, **keywords):
    try:
        return function(*arguments, **keywords)
    except records.InventoryError as error:
        raise ColdReceipt(str(error)) from error


def canonical(value):
    return _receipt(records.canonical, value)


def relative(path):
    return _receipt(records.relative, path)


def _stable(metadata):
    return records.stable(metadata)


def _root_descriptor(root):
    return _receipt(filesystem.root_descriptor, root)


class HardlinkInventory(leaves.HardlinkInventory):
    """Receipt error boundary; inode admission belongs to the canonical engine."""
    def observe(self, metadata, digest, physical, logical=None, raw_metadata=None):
        return _receipt(super().observe, metadata, digest, physical, logical, raw_metadata)

    def finish(self):
        return _receipt(super().finish)


def _entry(directory, name, path, root):
    return _entry_at(directory, name, path, os.path.join(root, path))


def _entry_at(directory, name, path, physical, hardlinks=None):
    return _receipt(leaves.entry_at, directory, name, path, physical, hardlinks)


def _links(entries, structural_prefixes=frozenset()):
    return _receipt(records.links, entries, structural_prefixes)


def _prefixes(roots):
    return records.prefixes(roots)


def manifest(root, allowed_roots):
    """Complete quarantine observation; bytes alone grant no receipt authority."""
    return _receipt(_inventory, root, allowed_roots, (), False).canonical_bytes


def inventory_exact_roots(root, allowed_roots, optional_roots=()):
    """Selected roots cover children; structural ancestor metadata is unclaimed."""
    return _receipt(_inventory, root, allowed_roots, optional_roots, True).canonical_bytes
