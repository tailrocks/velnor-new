# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import re
import stat
import tempfile

MAX_ARCHIVE_BYTES = 4 * 1024**3


class AuthorityError(ValueError):
    pass


@contextmanager
def snapshot_artifact(path, descriptor, private_parent):
    if (
        not isinstance(descriptor, dict)
        or set(descriptor) != {"sha256", "size_bytes"}
        or not isinstance(descriptor["sha256"], str)
        or re.fullmatch("[0-9a-f]{64}", descriptor["sha256"]) is None
        or type(descriptor["size_bytes"]) is not int
        or not 0 < descriptor["size_bytes"] <= MAX_ARCHIVE_BYTES
    ):
        raise AuthorityError("Artifact descriptor is invalid.")
    try:
        parent = Path(private_parent)
        metadata = parent.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
            raise AuthorityError("Artifact scratch directory is not private and owned.")
        descriptor_fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(descriptor_fd, "rb") as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size != descriptor["size_bytes"]:
                raise AuthorityError("Artifact is not a regular file of the signed size.")
            with tempfile.TemporaryDirectory(prefix="runner-authority-", dir=parent) as directory:
                snapshot = Path(directory) / "artifact"
                digest = hashlib.sha256()
                count = 0
                with snapshot.open("xb") as destination:
                    while chunk := source.read(1024**2):
                        count += len(chunk)
                        if count > descriptor["size_bytes"]:
                            raise AuthorityError("Artifact grew beyond its signed size.")
                        digest.update(chunk)
                        destination.write(chunk)
                if count != descriptor["size_bytes"] or digest.hexdigest() != descriptor["sha256"]:
                    raise AuthorityError("Artifact differs from its signed bytes.")
                snapshot.chmod(0o400)
                yield snapshot
    except OSError as error:
        raise AuthorityError("Cannot hold a private artifact snapshot.") from error
