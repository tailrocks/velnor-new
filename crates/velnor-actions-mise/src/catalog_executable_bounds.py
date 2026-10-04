"""Compiled security resource limits; these do not qualify artifact sizes."""
import hashlib

MAX_EXECUTABLE_BYTES = 256 * 1024 * 1024


def executable_limit():
    return MAX_EXECUTABLE_BYTES


def archive_limit():
    return executable_limit() + 32 * 1024 * 1024


def stream_sha256(stream):
    digest = hashlib.sha256()
    total = 0
    while True:
        block = stream.read(min(1024 * 1024, executable_limit() + 1 - total))
        if not block:
            return digest.hexdigest()
        total += len(block)
        if total > executable_limit():
            raise ValueError('executable_resource_limit')
        digest.update(block)
