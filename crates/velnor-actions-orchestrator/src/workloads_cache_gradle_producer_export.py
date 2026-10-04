"""Export only native Gradle build-cache keys from the trusted producer.

Gradle creates metadata and lock files beside native cache entries.  The
archive directory is separate, so those names never reach the consumer cache.
"""
import os
from pathlib import Path
import re
import stat


ALLOWED_METADATA = {"gc.properties", "native-cache-working.lock"}
MAX_ENTRIES = 4096
MAX_BYTES = 512 * 1024 * 1024
KEY_NAME = re.compile(r"^[0-9a-f]{32}$")


def checked_directory(path):
    info = os.lstat(path)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise ValueError("redirected cache directory")
    return path


def stable(before, after, size):
    return (stat.S_ISREG(after.st_mode) and after.st_nlink == 1
            and after.st_dev == before.st_dev and after.st_ino == before.st_ino
            and after.st_size == size and after.st_mtime_ns == before.st_mtime_ns
            and after.st_ctime_ns == before.st_ctime_ns)


def read_file(path, limit):
    before = os.lstat(path)
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
        raise ValueError("redirected or oversized cache file")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    body = bytearray()
    try:
        opened = os.fstat(descriptor)
        if (opened.st_dev != before.st_dev or opened.st_ino != before.st_ino
                or opened.st_nlink != 1 or opened.st_size > limit):
            raise ValueError("cache file identity changed")
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            body.extend(chunk)
            if len(body) > limit:
                raise ValueError("native cache byte budget")
        current = os.fstat(descriptor)
        if not stable(before, current, len(body)):
            raise ValueError("cache file changed while reading")
        return bytes(body)
    finally:
        os.close(descriptor)


def write_file(path, body):
    if path.exists() or path.is_symlink():
        raise ValueError("duplicate cache output")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        offset = 0
        while offset < len(body):
            written = os.write(descriptor, body[offset:])
            if written <= 0:
                raise ValueError("native cache copy stalled")
            offset += written
        generated = os.fstat(descriptor)
        if (not stat.S_ISREG(generated.st_mode) or generated.st_nlink != 1
                or generated.st_size != len(body)):
            raise ValueError("generated cache file changed")
    finally:
        os.close(descriptor)
    return len(body)


def ensure_directory(path):
    missing = []
    current = path
    while not current.exists() and not current.is_symlink():
        missing.append(current)
        current = current.parent
    checked_directory(current)
    for directory in reversed(missing):
        directory.mkdir(mode=0o700)
        checked_directory(directory)
    return path


def reject_links_under(path, base):
    if not path.is_absolute() or not base.is_absolute():
        raise ValueError("cache path is not absolute")
    if base.resolve() != base:
        raise ValueError("redirected runner temporary directory")
    if base not in path.parents and path != base:
        raise ValueError("cache path outside runner")
    current = path
    while True:
        if current.is_symlink():
            raise ValueError("redirected cache ancestor")
        if current == base:
            return
        current = current.parent


def copy_key(source, target, limit):
    info = os.lstat(source)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError("native cache key is not a private regular file")
    body = read_file(source, min(limit, MAX_COMPRESSED_BYTES))
    validate_key_archive(body)
    return write_file(target, body)


def main():
    working = Path(os.environ["VELNOR_GRADLE_PRODUCER_WORKING"])
    runner = Path(os.environ["RUNNER_TEMP"])
    reject_links_under(working, runner)
    checked_directory(runner)
    checked_directory(working)
    output = Path(os.environ["VELNOR_GRADLE_PRODUCER_OUTPUT"])
    reject_links_under(output, runner)
    if output.exists() or output.is_symlink():
        raise ValueError("cache export already exists")
    ensure_directory(output.parent)
    output.mkdir(mode=0o700)
    checked_directory(output)
    scanned = 0
    scanned_bytes = 0
    entries = []
    for entry in working.iterdir():
        scanned += 1
        if scanned > MAX_ENTRIES:
            raise ValueError("native cache directory entry budget")
        if entry.name == EXPECTED_KEY:
            info = os.lstat(entry)
            if (stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode)
                    or info.st_nlink != 1):
                raise ValueError("native cache key type")
            if info.st_size > MAX_BYTES - scanned_bytes:
                raise ValueError("native cache byte budget")
            scanned_bytes += info.st_size
            entries.append(entry)
        elif KEY_NAME.fullmatch(entry.name):
            info = os.lstat(entry)
            if (stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode)
                    or info.st_nlink != 1):
                raise ValueError("native cache key type")
            if info.st_size > MAX_BYTES - scanned_bytes:
                raise ValueError("native cache byte budget")
            scanned_bytes += info.st_size
        elif entry.name in ALLOWED_METADATA:
            info = os.lstat(entry)
            if (stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode)
                    or info.st_nlink != 1):
                raise ValueError("native cache metadata type")
            if info.st_size > MAX_BYTES - scanned_bytes:
                raise ValueError("native cache byte budget")
            scanned_bytes += info.st_size
        else:
            raise ValueError("unsupported native cache entry")
    if len(entries) != 1:
        raise ValueError("reviewed Java compile key missing")
    total = 0
    for entry in sorted(entries, key=lambda item: item.name):
        info = os.lstat(entry)
        if stat.S_ISREG(info.st_mode) and info.st_size > MAX_BYTES - total:
            raise ValueError("native cache byte budget")
        total += copy_key(entry, output / entry.name, MAX_BYTES - total)
    exported = []
    for entry in output.iterdir():
        if len(exported) >= 2:
            raise ValueError("unsafe cache export entry budget")
        exported.append(entry)
    if len(exported) != 1 or exported[0].name != EXPECTED_KEY:
        raise ValueError("unsafe cache export contents")
    info = os.lstat(exported[0])
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError("unsafe cache export file")
    validate_key_archive(read_file(exported[0], MAX_COMPRESSED_BYTES))


if __name__ == "__main__":
    main()
