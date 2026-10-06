"""Raw source observation primitives; no context minting or archive admission."""
import os

from source_archive_inventory import _finish
from source_archive_inventory_common import (
    InventoryError, _ORIGINAL_OBSERVATION, absolute, prefixes, recipe, stable,
)
from source_archive_inventory_leaf import HardlinkInventory, entry_at
from source_archive_inventory_walk import collect, walk


def _original_inventory(root, logical_roots, borrowed_descriptor):
    """Pure observation seam; source-owned adapter retains descriptor authority."""
    absolute(root)
    roots, optional = recipe(logical_roots)
    try:
        before = os.fstat(borrowed_descriptor)
        owner = entry_at(borrowed_descriptor, ".", ".", root,
                         observation=_ORIGINAL_OBSERVATION)[0]
        if owner["kind"] != "directory":
            raise InventoryError("source_original_owner_invalid")
        hardlinks = HardlinkInventory()
        entries, size = collect(walk(root, borrowed_descriptor, hardlinks, roots=roots,
                                     observation=_ORIGINAL_OBSERVATION))
        topology = hardlinks.topology()
        for entry in entries:
            if entry["kind"] in ("file", "symlink"):
                entry["hardlink"] = topology[entry["path"]]
        after = entry_at(borrowed_descriptor, ".", ".", root,
                         observation=_ORIGINAL_OBSERVATION)[0]
        if owner != after or stable(os.fstat(borrowed_descriptor)) != stable(before):
            raise InventoryError("source_original_owner_changed")
        owner_record = {"mode": owner["mode"], "metadata": owner["metadata"],
                        "local": owner["local"]}
        return _finish(entries, size, roots, optional, prefixes(roots), root, owner_record)
    except (OSError, RecursionError) as error:
        raise InventoryError("source_original_inventory_unavailable") from error
