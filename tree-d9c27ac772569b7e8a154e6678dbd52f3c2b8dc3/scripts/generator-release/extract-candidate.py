import os
import stat
import sys
import tarfile
import tempfile

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
    if not stat.S_ISREG(os.lstat(archive).st_mode):
        reject("candidate archive is not a regular file")
    if not stat.S_ISDIR(os.lstat(destination).st_mode):
        reject("candidate output is not a directory")
    if any(os.path.lexists(os.path.join(destination, name)) for name in expected):
        reject("candidate output already exists")
    archive_size = os.path.getsize(archive)
    if archive_size > maximum_archive_bytes or archive_size % 512:
        reject("candidate archive exceeds size limit or is truncated")

    with tarfile.open(archive, "r:") as bundle:
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
        with open(archive, "rb") as raw:
            raw.seek(offset)
            trailer = raw.read(maximum_trailer_bytes + 1)
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
