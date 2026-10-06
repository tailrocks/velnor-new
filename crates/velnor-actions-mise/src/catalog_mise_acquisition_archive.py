"""Closed, bounded tar.gz contract for an authenticated Mise release."""

import gzip
import hashlib
import io
import re
import tarfile
import unicodedata


MAX_MEMBERS = 4096
MAX_PATH_BYTES = 4096
MAX_COMPONENT_BYTES = 255


def _validate_path(name):
    if not isinstance(name, str) or not name or name.startswith("/"):
        raise ValueError("invalid archive member path")
    if re.match(r"^[A-Za-z]:", name):
        raise ValueError("invalid archive member path")
    if "\\" in name or any(unicodedata.category(c).startswith("C") for c in name):
        raise ValueError("invalid archive member path")
    parts = name.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise ValueError("invalid archive member path")
    if len(name.encode("utf-8")) > MAX_PATH_BYTES:
        raise ValueError("archive member path exceeds limit")
    if any(len(part.encode("utf-8")) > MAX_COMPONENT_BYTES for part in parts):
        raise ValueError("archive member component exceeds limit")


def _validate_declared_path(name, directory):
    _validate_path(name.removesuffix("/") if directory else name)


class _StrictTarInfo(tarfile.TarInfo):
    @classmethod
    def _validate_header(cls, buf, encoding, errors):
        if len(buf) != tarfile.BLOCKSIZE or buf == b"\0" * tarfile.BLOCKSIZE:
            return
        kind = buf[156:157]
        if kind not in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE):
            return
        name = tarfile.nts(buf[:100], encoding, errors)
        prefix = tarfile.nts(buf[345:500], encoding, errors)
        if prefix:
            name = prefix + "/" + name
        _validate_declared_path(name, kind == tarfile.DIRTYPE)

    @classmethod
    def frombuf(cls, buf, encoding, errors):
        cls._validate_header(buf, encoding, errors)
        return super().frombuf(buf, encoding, errors)

    @classmethod
    def _frombuf(cls, buf, encoding, errors, **kwargs):
        cls._validate_header(buf, encoding, errors)
        return super()._frombuf(buf, encoding, errors, **kwargs)

    def _apply_pax_info(self, pax_headers, encoding, errors):
        if any(key.startswith("GNU.sparse.") for key in pax_headers):
            raise ValueError("sparse archive member is unsupported")
        if "path" in pax_headers:
            _validate_declared_path(pax_headers["path"], self.type == tarfile.DIRTYPE)
        super()._apply_pax_info(pax_headers, encoding, errors)

    def _proc_gnusparse_00(self, *args):
        raise ValueError("sparse archive member is unsupported")

    def _proc_gnusparse_01(self, *args):
        raise ValueError("sparse archive member is unsupported")

    def _proc_gnusparse_10(self, *args):
        raise ValueError("sparse archive member is unsupported")


def _bounded_tar(archive):
    with gzip.GzipFile(fileobj=io.BytesIO(archive), mode="rb") as compressed:
        contents = compressed.read(archive_limit() + 1)
    if len(contents) > archive_limit():
        raise ValueError("decompressed archive exceeds limit")
    return tarfile.open(fileobj=io.BytesIO(contents), mode="r:", tarinfo=_StrictTarInfo)


def _read_member(tar, member):
    stream = tar.extractfile(member)
    if stream is None:
        raise ValueError("binary archive member is unreadable")
    with stream:
        binary = stream.read(member.size + 1)
    if len(binary) != member.size:
        raise ValueError("binary archive member has incorrect size")
    return binary


def extract_binary(archive: bytes, expected_binary_sha256: str, expected_member: str) -> bytes:
    """Return exactly the source-qualified regular member after digest validation."""
    _validate_path(expected_member)
    if not isinstance(expected_binary_sha256, str) or not re.fullmatch(
        r"[0-9a-f]{64}", expected_binary_sha256
    ):
        raise ValueError("invalid expected binary SHA-256")
    seen = set()
    regular_bytes = 0
    binary = None
    try:
        with _bounded_tar(archive) as tar:
            for count, member in enumerate(tar, start=1):
                if count > MAX_MEMBERS:
                    raise ValueError("archive member count exceeds limit")
                _validate_path(member.name)
                if member.name in seen:
                    raise ValueError("duplicate archive member")
                seen.add(member.name)
                if member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE):
                    raise ValueError("unsupported archive member type")
                if member.sparse is not None:
                    raise ValueError("sparse archive member is unsupported")
                if member.isdir():
                    if member.size != 0 or member.name == expected_member:
                        raise ValueError("invalid archive directory")
                    continue
                regular_bytes += member.size
                if member.size < 0 or regular_bytes > executable_limit():
                    raise ValueError("archive regular bytes exceed limit")
                if member.name == expected_member:
                    binary = _read_member(tar, member)
    except (tarfile.TarError, OSError, EOFError) as error:
        raise ValueError("invalid tar.gz archive") from error
    if binary is None:
        raise ValueError("expected binary archive member is missing")
    if hashlib.sha256(binary).hexdigest() != expected_binary_sha256:
        raise ValueError("binary SHA-256 mismatch")
    return binary
