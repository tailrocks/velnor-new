"""One mode, metadata, content and complete-hardlink admission implementation."""
import os
import stat

from opaque_inventory_metadata import metadata_records
from source_archive_inventory_common import InventoryError, _ORIGINAL_OBSERVATION, stable
from source_archive_inventory_fs import read_file


class HardlinkInventory:
    """Flatten only after all aliases of each physical inode were observed."""
    def __init__(self):
        self._groups = {}

    def observe(self, metadata, digest, physical, logical=None, raw_metadata=None):
        key = metadata.st_dev, metadata.st_ino
        identity = metadata.st_nlink, metadata.st_mode, metadata.st_size, digest, raw_metadata
        group = self._groups.setdefault(key, {"identity": identity, "paths": []})
        if group["identity"] != identity:
            raise InventoryError("payload_hardlink_inconsistent")
        group["paths"].append((physical, stable(metadata), logical))

    def finish(self):
        for group in self._groups.values():
            if len(group["paths"]) != group["identity"][0]:
                raise InventoryError("payload_hardlink_external_or_missing")
            for physical, observed, _logical in group["paths"]:
                if stable(os.stat(physical, follow_symlinks=False)) != observed:
                    raise InventoryError("payload_hardlink_changed")

    def topology(self):
        self.finish()
        result = {}
        for group in self._groups.values():
            logical = [path for _physical, _observed, path in group["paths"]]
            if any(path is None for path in logical):
                raise InventoryError("payload_hardlink_logical_missing")
            identity = {"representative": min(logical), "count": len(logical)}
            result.update((path, identity) for path in logical)
        return result


def entry_at(directory, name, path, physical, hardlinks=None, observation=None):
    original = observation is _ORIGINAL_OBSERVATION
    if observation is not None and not original:
        raise InventoryError("payload_observation_unqualified")
    metadata = os.stat(name, dir_fd=directory, follow_symlinks=False)
    try:
        records = metadata_records(physical, metadata, "record" if original else "reject")
    except ValueError as error:
        raise InventoryError("payload_unsupported_metadata") from error
    mode = stat.S_IMODE(metadata.st_mode)
    entry = {"path": path, "mode": mode, "sha256": None, "target": None}
    if original:
        entry.update(metadata=[record.hex() for record in records], hardlink=None,
                     local={"dev": metadata.st_dev, "ino": metadata.st_ino,
                            "uid": metadata.st_uid, "gid": metadata.st_gid,
                            "mtime_ns": metadata.st_mtime_ns, "ctime_ns": metadata.st_ctime_ns})
    size = 0
    if mode & 0o7000 and not original:
        raise InventoryError("payload_special_mode")
    if stat.S_ISREG(metadata.st_mode):
        entry["kind"] = "file"
        entry["sha256"], size = read_file(directory, name, metadata, hardlinks is not None,
                                         observation=observation)
        if hardlinks is not None:
            hardlinks.observe(metadata, entry["sha256"], physical, path,
                              tuple(records) if original else None)
    elif stat.S_ISDIR(metadata.st_mode):
        entry["kind"] = "directory"
    elif stat.S_ISLNK(metadata.st_mode):
        if not original and metadata.st_nlink != 1:
            raise InventoryError("payload_symlink_hardlink_unsupported")
        entry["kind"] = "symlink"
        entry["target"] = os.readlink(name, dir_fd=directory)
        if original and hardlinks is not None:
            hardlinks.observe(metadata, entry["target"], physical, path, tuple(records))
    else:
        raise InventoryError("payload_special_entry")
    if stable(os.stat(name, dir_fd=directory, follow_symlinks=False)) != stable(metadata):
        raise InventoryError("payload_entry_changed")
    return entry, size
