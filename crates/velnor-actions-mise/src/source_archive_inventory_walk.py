"""One descriptor traversal for live and numbered source-owned namespaces."""
import os
import stat

from source_archive_inventory_common import MAX_BYTES, MAX_ENTRIES, InventoryError, relative
from source_archive_inventory_leaf import entry_at


def walk(physical, boundary, hardlinks, logical_prefix="", roots=None, exact=True,
         observation=None):
    for directory, directories, files, descriptor in os.fwalk(
            ".", dir_fd=boundary, follow_symlinks=False):
        prefix = directory.removeprefix("./")
        admitted_directories = []
        for name in sorted(directories + files):
            tail = name if prefix == "." else prefix + "/" + name
            path = relative(logical_prefix + "/" + tail if logical_prefix else tail)
            if roots is not None:
                admitted = any(path == root or path.startswith(root + "/") for root in roots)
                structural = any(root.startswith(path + "/") for root in roots)
                if not admitted and not structural:
                    if exact:
                        continue
                    raise InventoryError("payload_unknown_path")
                if exact and structural and not admitted:
                    metadata = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
                    if not stat.S_ISDIR(metadata.st_mode):
                        raise InventoryError("payload_structural_prefix_invalid")
                    admitted_directories.append(name)
                    continue
            entry, size = entry_at(descriptor, name, path, os.path.join(physical, tail),
                                   hardlinks, observation)
            if name in directories:
                admitted_directories.append(name)
            yield entry, size
        directories[:] = admitted_directories


def collect(observations, entries=None, size=0):
    entries = [] if entries is None else entries
    for entry, added in observations:
        entries.append(entry)
        size += added
        if len(entries) > MAX_ENTRIES or size > MAX_BYTES:
            raise InventoryError("payload_inventory_limit")
    return entries, size


def root_entries(quarantine, boundary, index, logical, hardlinks):
    physical = os.path.join(quarantine, "roots", str(index))
    entry, size = entry_at(boundary, str(index), logical, physical, hardlinks)
    yield entry, size
    if entry["kind"] == "directory":
        descriptor = os.open(str(index), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
                             | os.O_CLOEXEC, dir_fd=boundary)
        try:
            yield from walk(physical, descriptor, hardlinks, logical)
        finally:
            os.close(descriptor)
