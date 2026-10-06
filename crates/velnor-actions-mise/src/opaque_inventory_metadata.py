"""Read metadata without following symlinks; expose only opaque digests."""
import ctypes
import errno
import fcntl
import hashlib
import os
import stat
import sys
def _metadata_digest(kind, name, value):
    digest = hashlib.sha256()
    for part in (kind, name, value):
        digest.update(len(part).to_bytes(8, "big"))
        digest.update(part)
    return digest.digest()
def _metadata_darwin_library():
    library = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
    pointer = ctypes.c_void_p
    signatures = {
        "listxattr": ([ctypes.c_char_p, pointer, ctypes.c_size_t, ctypes.c_int], ctypes.c_ssize_t),
        "getxattr": ([ctypes.c_char_p, ctypes.c_char_p, pointer, ctypes.c_size_t,
                      ctypes.c_uint32, ctypes.c_int], ctypes.c_ssize_t),
        "acl_get_link_np": ([ctypes.c_char_p, ctypes.c_int], pointer),
        "acl_get_entry": ([pointer, ctypes.c_int, ctypes.POINTER(pointer)], ctypes.c_int),
        "acl_to_text": ([pointer, ctypes.POINTER(ctypes.c_ssize_t)], pointer),
        "acl_free": ([pointer], ctypes.c_int),
    }
    for name, (arguments, result) in signatures.items():
        function = getattr(library, name)
        function.argtypes, function.restype = arguments, result
    return library
_METADATA_DARWIN = _metadata_darwin_library() if sys.platform == "darwin" else None
def _metadata_darwin_xattrs(path, reject):
    library = _METADATA_DARWIN
    # Include the compression attribute hidden by the default Darwin API.
    options = 0x0001 | 0x0020
    size = library.listxattr(path, None, 0, options)
    if size < 0 or (reject and size):
        raise ValueError("unsupported filesystem metadata")
    if not size:
        return []
    buffer = ctypes.create_string_buffer(size)
    if library.listxattr(path, buffer, size, options) != size:
        raise ValueError("unstable filesystem metadata")
    names = buffer.raw.split(b"\0")
    if names[-1] or any(not name for name in names[:-1]):
        raise ValueError("invalid filesystem metadata")
    records = []
    for name in sorted(names[:-1]):
        size = library.getxattr(path, name, None, 0, 0, options)
        if size < 0:
            raise ValueError("unreadable filesystem metadata")
        buffer = ctypes.create_string_buffer(size)
        if library.getxattr(path, name, buffer, size, 0, options) != size:
            raise ValueError("unstable filesystem metadata")
        records.append(_metadata_digest(b"xattr", name, buffer.raw))
    return records
def _metadata_darwin_acl(path, reject):
    library = _METADATA_DARWIN
    ctypes.set_errno(0)
    acl = library.acl_get_link_np(path, 0x00000100)
    if not acl:
        if ctypes.get_errno() == errno.ENOENT:
            return []
        raise ValueError("unreadable filesystem metadata")
    try:
        entry = ctypes.c_void_p()
        ctypes.set_errno(0)
        result = library.acl_get_entry(acl, 0, ctypes.byref(entry))
        if result == -1 and ctypes.get_errno() == errno.EINVAL:
            return []
        if result != 0 or reject:
            raise ValueError("unsupported filesystem metadata")
        length = ctypes.c_ssize_t()
        text = library.acl_to_text(acl, ctypes.byref(length))
        if not text:
            raise ValueError("unreadable filesystem metadata")
        try:
            if length.value < 0:
                raise ValueError("invalid filesystem metadata")
            value = ctypes.string_at(text, length.value)
            return [_metadata_digest(b"acl", b"", value)]
        finally:
            if library.acl_free(text) != 0:
                raise ValueError("unreadable filesystem metadata")
    finally:
        if library.acl_free(acl) != 0:
            raise ValueError("unreadable filesystem metadata")
def _metadata_linux_flags(path, info):
    if not (stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode)):
        return 0
    if os.uname().machine not in ("x86_64", "aarch64"):
        raise ValueError("unsupported metadata platform")
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        actual = os.fstat(descriptor)
        if (actual.st_dev, actual.st_ino, actual.st_mode) != (info.st_dev, info.st_ino, info.st_mode):
            raise ValueError("unstable filesystem metadata")
        value = bytearray(4)
        request = 0x80006601 | (ctypes.sizeof(ctypes.c_long) << 16)
        fcntl.ioctl(descriptor, request, value)
        return int.from_bytes(value, sys.byteorder)
    finally:
        os.close(descriptor)
def metadata_records(path, info, policy):
    """Return fixed length opaque records, or reject exportable metadata."""
    if policy not in ("record", "reject"):
        raise ValueError("invalid metadata policy")
    reject = policy == "reject"
    try:
        flags = _metadata_linux_flags(path, info) if sys.platform == "linux" else getattr(info, "st_flags", 0)
        if reject and flags:
            raise ValueError("unsupported filesystem metadata")
        records = []
        if flags:
            records.append(_metadata_digest(b"flags", b"", str(flags).encode("ascii")))
        if sys.platform == "darwin":
            raw = os.fsencode(path)
            records.extend(_metadata_darwin_xattrs(raw, reject))
            records.extend(_metadata_darwin_acl(raw, reject))
        elif sys.platform == "linux":
            names = os.listxattr(path, follow_symlinks=False)
            if reject and names:
                raise ValueError("unsupported filesystem metadata")
            for name in sorted(names, key=os.fsencode):
                value = os.getxattr(path, name, follow_symlinks=False)
                records.append(_metadata_digest(b"xattr", os.fsencode(name), value))
        else:
            raise ValueError("unsupported metadata platform")
    except OSError:
        raise ValueError("unreadable filesystem metadata") from None
    return sorted(records)
