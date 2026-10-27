# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
from pathlib import PurePosixPath
import zlib

MAX_ARCHIVE_CONTENT_BYTES = 4 * 1024**3
MAX_TAR_EXTENSION_BYTES = 1024**2
MAX_TAR_EXTENSION_TOTAL_BYTES = 4 * 1024**2


class AuthorityError(ValueError):
    pass


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _hash_stream(stream, byte_limit=None):
    digest = hashlib.sha256()
    size = 0
    while chunk := stream.read(1024 * 1024):
        digest.update(chunk)
        size += len(chunk)
        if byte_limit is not None and size > byte_limit:
            raise AuthorityError("OCI archive content exceeds its size limit.")
    return digest.hexdigest(), size


def _safe_member(name):
    path = PurePosixPath(name)
    normalized = str(path)
    if path.is_absolute() or ".." in path.parts or normalized != name or name in {"", "."}:
        raise AuthorityError("OCI archive contains a path that is not safe.")
    return name


def _tar_member_size(header):
    field = header[124:136]
    if field[0] & 0x80:
        value = int.from_bytes(field, "big", signed=False) & ((1 << (8 * len(field) - 1)) - 1)
    else:
        try:
            value = int(field.strip(b"\0 ") or b"0", 8)
        except ValueError as error:
            raise AuthorityError("Tar archive has an incorrect member size.") from error
    if value < 0:
        raise AuthorityError("Tar archive has a negative member size.")
    return value


def _scan_tar_extensions(stream):
    extension_total = 0
    while header := stream.read(512):
        if len(header) != 512:
            return
        if header == bytes(512):
            return
        size = _tar_member_size(header)
        if header[156:157] in {b"x", b"g", b"L", b"K"}:
            if size > MAX_TAR_EXTENSION_BYTES:
                raise AuthorityError("Tar extension header exceeds its size limit.")
            extension_total += size
            if extension_total > MAX_TAR_EXTENSION_TOTAL_BYTES:
                raise AuthorityError("Tar extension headers exceed their total size limit.")
        remaining = (size + 511) // 512 * 512
        while remaining:
            chunk = stream.read(min(1024 * 1024, remaining))
            if not chunk:
                return
            remaining -= len(chunk)


def _scan_archive_extensions(path):
    try:
        with path.open("rb") as source:
            prefix = source.read(6)
            if prefix.startswith((b"\x1f\x8b", b"BZh", b"\xfd7zXZ\x00")):
                raise AuthorityError("OCI archive must use an uncompressed tar format.")
            source.seek(0)
            _scan_tar_extensions(_LayerHashReader(source, MAX_ARCHIVE_CONTENT_BYTES))
    except (OSError, EOFError, zlib.error) as error:
        raise AuthorityError("OCI archive extension scan failed.") from error


def _layer_path(name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts:
        raise AuthorityError("OCI layer has a path that is not safe.")
    normalized = str(path)
    if normalized in {"", "."}:
        return "."
    return normalized


class _LayerHashReader:
    def __init__(self, stream, byte_limit):
        self.stream = stream
        self.byte_limit = byte_limit
        self.digest = hashlib.sha256()
        self.size = 0

    def read(self, size=-1):
        data = self.stream.read(size)
        self.size += len(data)
        if self.size > self.byte_limit:
            raise AuthorityError("OCI image layers exceed their uncompressed size limit.")
        self.digest.update(data)
        return data
