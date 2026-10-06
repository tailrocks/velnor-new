"""Rooted descriptor reads shared by every source archive observer."""
import hashlib
import os
import stat

from metadata_container import is_appledouble
from source_archive_inventory_common import (
    MAX_BYTES, InventoryError, _ORIGINAL_OBSERVATION, absolute, stable,
)


def root_descriptor(root):
    components = absolute(root)
    descriptor = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for component in components:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
                            | os.O_CLOEXEC, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        return descriptor
    except (OSError, ValueError):
        os.close(descriptor)
        raise InventoryError("payload_root_unqualified") from None


def read_file(directory, name, expected, admitted_links=False, consume=None, observation=None):
    """Hash stable regular bytes; an optional sink consumes those same chunks."""
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
                         | os.O_CLOEXEC, dir_fd=directory)
    try:
        return read_descriptor(descriptor, expected, admitted_links, consume, observation)
    finally:
        os.close(descriptor)


def read_descriptor(descriptor, expected, admitted_links=False, consume=None, observation=None):
    """Consume an already rooted descriptor while preserving caller ownership."""
    digest, size = hashlib.sha256(), 0
    with os.fdopen(os.dup(descriptor), "rb") as stream:
        before = os.fstat(stream.fileno())
        if (not stat.S_ISREG(before.st_mode) or stable(before) != stable(expected)
                or (not admitted_links and before.st_nlink != 1)):
            raise InventoryError("payload_file_changed_or_hardlinked")
        while chunk := stream.read(1024 * 1024):
            if (size == 0 and observation is not _ORIGINAL_OBSERVATION
                    and is_appledouble(chunk, before.st_size)):
                raise InventoryError("payload_appledouble_metadata")
            size += len(chunk)
            if size > MAX_BYTES:
                raise InventoryError("payload_byte_limit")
            digest.update(chunk)
            if consume is not None:
                consume(chunk)
        if size != before.st_size or stable(os.fstat(stream.fileno())) != stable(before):
            raise InventoryError("payload_file_changed")
    return digest.hexdigest(), size
