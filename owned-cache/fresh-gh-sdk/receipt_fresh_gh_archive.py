"""Bounded gzip/tar parsing; exactly one SDK-selected regular executable."""
import gzip
import hashlib
import io
import os
import tarfile
import tempfile
import time
import zlib

from cache_receipt_common import ColdReceipt

_EXPANDED_LIMIT = 256 * 1024 * 1024
_BINARY_LIMIT = 64 * 1024 * 1024
_MEMBER_LIMIT = 4096
_DEADLINE = 30


def _expand(archive, output, deadline):
    total = 0
    with gzip.GzipFile(fileobj=io.BytesIO(archive), mode='rb') as compressed:
        while True:
            if time.monotonic() >= deadline:
                raise ColdReceipt('fresh_gh_archive_timeout')
            data = compressed.read(min(65536, _EXPANDED_LIMIT - total + 1))
            if not data:
                break
            total += len(data)
            if total > _EXPANDED_LIMIT:
                raise ColdReceipt('fresh_gh_expanded_size')
            output.write(data)
    output.seek(0)


def _name(member):
    name = member.name.rstrip('/') if member.isdir() else member.name
    if (not name or name.startswith('/') or '\\' in name
            or any(part in ('', '.', '..') for part in name.split('/'))
            or not (member.isdir() or member.isfile()) or member.sparse is not None
            or member.pax_headers):
        raise ColdReceipt('fresh_gh_member_shape')
    return name


def _binary(archive, record, deadline):
    found, names = None, set()
    with tarfile.open(fileobj=archive, mode='r:') as contents:
        for member in contents:
            if time.monotonic() >= deadline:
                raise ColdReceipt('fresh_gh_archive_timeout')
            name = _name(member)
            if name in names or len(names) >= _MEMBER_LIMIT:
                raise ColdReceipt('fresh_gh_member_count')
            names.add(name)
            if name != record['binary_member']:
                continue
            if not member.isfile() or not 0 < member.size <= _BINARY_LIMIT:
                raise ColdReceipt('fresh_gh_binary_shape')
            stream = contents.extractfile(member)
            if stream is None:
                raise ColdReceipt('fresh_gh_binary_missing')
            with stream:
                found = stream.read(_BINARY_LIMIT + 1)
            if len(found) != member.size:
                raise ColdReceipt('fresh_gh_binary_size')
    if found is None or hashlib.sha256(found).hexdigest() != record['binary_sha256']:
        raise ColdReceipt('fresh_gh_binary_digest')
    return found


def extract(archive, record, home):
    """No tar extraction, shell aliases, gzip executable or restored files."""
    deadline = time.monotonic() + _DEADLINE
    try:
        with tempfile.TemporaryFile(dir=home) as expanded:
            _expand(archive, expanded, deadline)
            binary = _binary(expanded, record, deadline)
        path = home + '/gh'
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o500)
        with os.fdopen(descriptor, 'wb') as output:
            output.write(binary)
        return path
    except (OSError, EOFError, tarfile.TarError, zlib.error) as error:
        raise ColdReceipt('fresh_gh_archive_invalid') from error
