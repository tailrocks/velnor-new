import io
import os
import stat
import sys
import tarfile
import tempfile

scripts_directory = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, scripts_directory)
from owned_archive_preflight import preflight_archive

archive, destination, executable, checksum, provenance = sys.argv[1:]
expected = [executable, checksum, provenance]
maximum_bytes = 256 * 1024 * 1024
maximum_trailer_bytes = 16 * 1024
maximum_archive_bytes = maximum_bytes + maximum_trailer_bytes
temporary = []
published = []


def reject(reason):
    raise ValueError(reason)


try:
    if not stat.S_ISDIR(os.lstat(destination).st_mode):
        reject("candidate output is not a directory")
    if any(os.path.lexists(os.path.join(destination, name)) for name in expected):
        reject("candidate output already exists")
    if not hasattr(os, "O_NOFOLLOW"):
        reject("candidate archive requires no-follow file support")
    descriptor = os.open(archive, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            reject("candidate archive is not a regular file")
        archive_size = before.st_size
        if archive_size > maximum_archive_bytes or archive_size % 512:
            reject("candidate archive exceeds size limit or is truncated")
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            archive_bytes = source.read(maximum_archive_bytes + 1)
        after = os.fstat(descriptor)
        named = os.stat(archive, follow_symlinks=False)
        identity_before = (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                           before.st_ctime_ns, before.st_mode, before.st_nlink)
        identity_after = (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                          after.st_ctime_ns, after.st_mode, after.st_nlink)
        if identity_before != identity_after or (before.st_dev, before.st_ino) != (
                named.st_dev, named.st_ino):
            reject("candidate archive changed while reading")
    finally:
        os.close(descriptor)
    if len(archive_bytes) != archive_size:
        reject("candidate archive changed while reading")
    if archive_size > maximum_archive_bytes or archive_size % 512:
        reject("candidate archive exceeds size limit or is truncated")
    preflight_archive(archive_bytes, "candidate")

    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:") as bundle:
        members = []
        while len(members) <= len(expected):
            member = bundle.next()
            if member is None:
                break
            members.append(member)
        if len(members) != len(expected):
            reject("unexpected candidate archive member count")
        if [member.name for member in members] != expected:
            reject("candidate archive members differ from expected order")

        offset = 0
        total_size = 0
        for index, member in enumerate(members):
            if member.offset != offset:
                reject("candidate archive contains an extra header")
            if member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE):
                reject("candidate archive contains a non-regular member")
            if member.pax_headers:
                reject("candidate archive contains extended metadata")
            mode = 0o755 if index == 0 else 0o644
            if member.mode != mode:
                reject("candidate archive member mode differs from policy")
            if member.size < 0 or member.size > maximum_bytes:
                reject("candidate archive member exceeds size limit")
            total_size += member.size
            if total_size > maximum_bytes:
                reject("candidate archive exceeds total size limit")
            offset = member.offset_data + ((member.size + 511) // 512) * 512

        trailer_size = archive_size - offset
        if trailer_size < 1024 or trailer_size > maximum_trailer_bytes or trailer_size % 512:
            reject("candidate archive has an invalid end marker")
        trailer = archive_bytes[offset:]
        if len(trailer) != trailer_size or any(trailer):
            reject("candidate archive has an invalid end marker")

        for index, (member, name) in enumerate(zip(members, expected)):
            source = bundle.extractfile(member)
            if source is None:
                reject("candidate archive member has no file content")
            descriptor, path = tempfile.mkstemp(prefix=".velnor-release-", dir=destination)
            temporary.append(path)
            remaining = member.size
            with os.fdopen(descriptor, "wb") as output:
                while remaining:
                    block = source.read(min(1024 * 1024, remaining))
                    if not block:
                        reject("candidate archive member is truncated")
                    output.write(block)
                    remaining -= len(block)
                if source.read(1):
                    reject("candidate archive member exceeds declared size")
            os.chmod(path, 0o755 if index == 0 else 0o644)

    if any(os.path.lexists(os.path.join(destination, name)) for name in expected):
        reject("candidate output appeared during extraction")
    for path, name in zip(temporary, expected):
        final = os.path.join(destination, name)
        os.link(path, final, follow_symlinks=False)
        published.append(final)
        os.unlink(path)
    temporary.clear()
    published.clear()
except Exception:
    for path in published:
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass
    for path in temporary:
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass
    raise
