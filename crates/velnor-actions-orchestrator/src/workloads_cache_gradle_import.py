"""Import only the sealed Gradle native cache keys into consumer state.

The producer archive is immutable transport.  The consumer cache is fresh for
this job and is never uploaded, so repository Gradle code cannot alter the
producer archive or another job's native outputs.
"""
import os
from pathlib import Path
import stat


MAX_ENTRIES = 4096
MAX_BYTES = 512 * 1024 * 1024


def checked_directory(path):
    info = os.lstat(path)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise ValueError("redirected Gradle cache directory")
    return path


def stable(before, after, size):
    return (stat.S_ISREG(after.st_mode) and after.st_nlink == 1
            and after.st_dev == before.st_dev and after.st_ino == before.st_ino
            and after.st_size == size and after.st_mtime_ns == before.st_mtime_ns
            and after.st_ctime_ns == before.st_ctime_ns)


def read_file(path, limit):
    before = os.lstat(path)
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
        raise ValueError("sealed cache entry is not a bounded regular file")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    body = bytearray()
    try:
        opened = os.fstat(descriptor)
        if (opened.st_dev != before.st_dev or opened.st_ino != before.st_ino
                or opened.st_nlink != 1 or opened.st_size > limit):
            raise ValueError("sealed cache entry identity changed")
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            body.extend(chunk)
            if len(body) > limit:
                raise ValueError("sealed cache entry byte budget")
        current = os.fstat(descriptor)
        if not stable(before, current, len(body)):
            raise ValueError("sealed cache entry changed while reading")
        return bytes(body)
    finally:
        os.close(descriptor)


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
    if not path.is_absolute() or not base.is_absolute() or base.resolve() != base:
        raise ValueError("redirected runner temporary directory")
    if base not in path.parents and path != base:
        raise ValueError("cache path outside runner")
    current = path
    while True:
        if current.is_symlink():
            raise ValueError("redirected Gradle cache ancestor")
        if current == base:
            return
        current = current.parent


def copy_file(source, target, limit):
    info = os.lstat(source)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError("sealed cache entry is not a private regular file")
    if info.st_size > limit or target.exists() or target.is_symlink():
        raise ValueError("sealed cache entry exceeds import boundary")
    body = read_file(source, min(limit, MAX_COMPRESSED_BYTES))
    validate_key_archive(body)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    target_fd = os.open(target, flags, 0o600)
    try:
        offset = 0
        while offset < len(body):
            written = os.write(target_fd, body[offset:])
            if written <= 0:
                raise ValueError("Gradle cache import stalled")
            offset += written
        generated = os.fstat(target_fd)
        if (not stat.S_ISREG(generated.st_mode) or generated.st_nlink != 1
                or generated.st_size != len(body)):
            raise ValueError("generated cache entry changed")
    finally:
        os.close(target_fd)
    return len(body)


def archive_entries(archive):
    if archive.is_symlink():
        raise ValueError("sealed Gradle cache archive is redirected")
    if not archive.exists():
        return []
    checked_directory(archive)
    entries = []
    total = 0
    for entry in archive.iterdir():
        if len(entries) >= MAX_ENTRIES:
            raise ValueError("sealed Gradle cache entry budget")
        if entry.name != EXPECTED_KEY:
            raise ValueError("sealed Gradle cache filename")
        info = os.lstat(entry)
        if (stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode)
                or info.st_nlink != 1):
            raise ValueError("sealed Gradle cache entry type")
        if info.st_size > MAX_COMPRESSED_BYTES or info.st_size > MAX_BYTES - total:
            raise ValueError("sealed Gradle cache byte budget")
        total += info.st_size
        entries.append(entry)
    return sorted(entries, key=lambda item: item.name)


def main():
    runner = Path(os.environ["RUNNER_TEMP"])
    home = Path(os.environ["GRADLE_USER_HOME"])
    archive = Path(os.environ["VELNOR_GRADLE_PRODUCER_OUTPUT"])
    destination = Path(os.environ["VELNOR_GRADLE_CONSUMER_CACHE"])
    expected_home = runner / "velnor" / "native" / "gradle"
    if (home != expected_home
            or archive != expected_home / "velnor-compile-export-v1"
            or destination != expected_home / "velnor-compile-cache-v1"):
        raise ValueError("closed Gradle import paths")
    reject_links_under(archive, runner)
    reject_links_under(destination, runner)
    checked_directory(runner)
    entries = archive_entries(archive)
    if destination.exists() or destination.is_symlink():
        raise ValueError("consumer Gradle cache is not fresh")
    ensure_directory(destination.parent)
    destination.mkdir(mode=0o700)
    checked_directory(destination)
    total = 0
    for entry in entries:
        total += copy_file(entry, destination / entry.name, MAX_BYTES - total)
    copied = []
    for entry in destination.iterdir():
        if len(copied) >= 2:
            raise ValueError("consumer Gradle cache entry budget")
        copied.append(entry)
    copied.sort(key=lambda item: item.name)
    if [entry.name for entry in copied] != [EXPECTED_KEY]:
        raise ValueError("consumer Gradle cache import changed")
    info = os.lstat(copied[0])
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError("consumer Gradle cache entry changed")
    validate_key_archive(read_file(copied[0], MAX_COMPRESSED_BYTES))


if __name__ == "__main__":
    main()
