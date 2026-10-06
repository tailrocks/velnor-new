"""Fixed Prepared payload custody and runner output; no source or SDK grant."""
import hashlib
import os
from pathlib import Path
import re
import stat


_PREPARED_DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC


def _prepared_directory_identity(descriptor, private=False):
    value = os.fstat(descriptor)
    require(stat.S_ISDIR(value.st_mode) and value.st_uid == os.getuid() and
            stat.S_IMODE(value.st_mode) & 0o022 == 0 and
            (not private or stat.S_IMODE(value.st_mode) == 0o700),
            "prepared_output_directory")
    return value.st_dev, value.st_ino, value.st_uid, value.st_gid, value.st_mode


def _prepared_payload_directories(root):
    descriptors = []
    try:
        root_descriptor = os.open(root, _PREPARED_DIRECTORY_FLAGS)
        descriptors.append(root_descriptor)
        root_identity = _prepared_directory_identity(root_descriptor)
        try:
            os.mkdir("velnor", mode=0o700, dir_fd=root_descriptor)
        except FileExistsError:
            pass
        parent = os.open("velnor", _PREPARED_DIRECTORY_FLAGS, dir_fd=root_descriptor)
        descriptors.append(parent)
        parent_identity = _prepared_directory_identity(parent)
        os.mkdir("source-intent-prepared", mode=0o700, dir_fd=parent)
        destination = os.open("source-intent-prepared", _PREPARED_DIRECTORY_FLAGS, dir_fd=parent)
        descriptors.append(destination)
        destination_identity = _prepared_directory_identity(destination, private=True)
        return ((root_descriptor, None, str(root), root_identity),
                (parent, root_descriptor, "velnor", parent_identity),
                (destination, parent, "source-intent-prepared", destination_identity))
    except BaseException:
        for descriptor in reversed(descriptors):
            os.close(descriptor)
        raise


def _prepared_require_anchors(anchors):
    for descriptor, parent, name, expected in anchors:
        require(_prepared_directory_identity(descriptor) == expected, "prepared_output_changed")
        value = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require((value.st_dev, value.st_ino, value.st_uid, value.st_gid, value.st_mode) == expected,
                "prepared_output_rebound")


def _prepared_payload_identity(value):
    return (value.st_dev, value.st_ino, value.st_uid, value.st_gid, value.st_mode,
            value.st_nlink, value.st_size, value.st_mtime_ns, value.st_ctime_ns)


def _prepared_require_payload(descriptor, parent, identity, digest):
    before = os.fstat(descriptor)
    require(_prepared_payload_identity(before) == identity, "prepared_output_payload_changed")
    linked = os.stat("prepared.zip", dir_fd=parent, follow_symlinks=False)
    require(_prepared_payload_identity(linked) == identity, "prepared_output_payload_rebound")
    os.lseek(descriptor, 0, os.SEEK_SET)
    observed, size = hashlib.sha256(), 0
    while chunk := os.read(descriptor, min(1024 * 1024, before.st_size + 1 - size)):
        size += len(chunk)
        require(size <= before.st_size, "prepared_output_payload_size")
        observed.update(chunk)
    require(size == before.st_size and observed.hexdigest() == digest and
            _prepared_payload_identity(os.fstat(descriptor)) == identity,
            "prepared_output_payload_changed")


class _PreparedOutputSink:
    __slots__ = ("_descriptor", "_anchors", "_digest", "_closed",
                 "_payload_descriptor", "_payload_identity")

    def __init__(self, seal, root):
        require(seal is _PREPARED_CONTEXT_SEAL, "prepared_output_capability")
        path = os.environ.get("GITHUB_OUTPUT")
        require(type(path) is str and os.path.isabs(path), "prepared_output_path")
        descriptor = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            require(stat.S_ISREG(os.fstat(descriptor).st_mode), "prepared_output_kind")
            anchors = _prepared_payload_directories(root)
        except BaseException:
            os.close(descriptor)
            raise
        for key, value in (("_descriptor", descriptor), ("_anchors", anchors),
                           ("_digest", None), ("_closed", False),
                           ("_payload_descriptor", None), ("_payload_identity", None)):
            object.__setattr__(self, key, value)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_prepared_output")

    def close(self):
        if not self._closed:
            object.__setattr__(self, "_closed", True)
            os.close(self._descriptor)
            if self._payload_descriptor is not None:
                os.close(self._payload_descriptor)
            for descriptor, _, _, _ in reversed(self._anchors):
                os.close(descriptor)

    def write_prepared_payload(self, raw):
        require(not self._closed and self._digest is None, "prepared_output_repeated")
        try:
            require(type(raw) is bytes, "prepared_output_bytes")
            _prepared_require_anchors(self._anchors)
            parent = self._anchors[-1][0]
            descriptor = os.open("prepared.zip", os.O_RDWR | os.O_CREAT | os.O_EXCL |
                                 os.O_NOFOLLOW | os.O_CLOEXEC, 0o400, dir_fd=parent)
            with os.fdopen(descriptor, "wb") as output:
                before = os.fstat(output.fileno())
                require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid() and
                        before.st_nlink == 1 and stat.S_IMODE(before.st_mode) == 0o400,
                        "prepared_output_file")
                output.write(raw)
                output.flush()
                os.fsync(output.fileno())
                after = os.fstat(output.fileno())
                require((before.st_dev, before.st_ino, before.st_uid, before.st_mode) ==
                        (after.st_dev, after.st_ino, after.st_uid, after.st_mode) and
                        after.st_nlink == 1 and after.st_size == len(raw),
                        "prepared_output_file_changed")
                linked = os.stat("prepared.zip", dir_fd=parent, follow_symlinks=False)
                require((linked.st_dev, linked.st_ino, linked.st_mode) ==
                        (after.st_dev, after.st_ino, after.st_mode), "prepared_output_file_rebound")
                object.__setattr__(self, "_payload_descriptor", os.dup(output.fileno()))
                object.__setattr__(self, "_payload_identity", _prepared_payload_identity(after))
            _prepared_require_anchors(self._anchors)
            digest = hashlib.sha256(raw).hexdigest()
            _prepared_require_payload(self._payload_descriptor, parent, self._payload_identity, digest)
            object.__setattr__(self, "_digest", digest)
            return digest
        except BaseException:
            self.close()
            raise

    def publish_prepared_sha256(self, digest):
        require(not self._closed, "prepared_output_repeated")
        try:
            require(type(digest) is str and re.fullmatch(r"[0-9a-f]{64}", digest) and
                    self._digest is not None and digest == self._digest, "prepared_output_digest")
            _prepared_require_anchors(self._anchors)
            _prepared_require_payload(self._payload_descriptor, self._anchors[-1][0],
                                      self._payload_identity, digest)
            raw = ("package-blob-sha256=" + digest + "\n").encode("ascii")
            require(os.write(self._descriptor, raw) == len(raw), "prepared_output_truncated")
            os.fsync(self._descriptor)
        finally:
            self.close()
