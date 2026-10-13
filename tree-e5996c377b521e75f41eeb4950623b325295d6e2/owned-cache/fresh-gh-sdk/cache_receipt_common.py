"""Shared bounded public receipt evidence utilities; no consumer authority."""
import json
import math
import os
import stat

MAX_EVIDENCE = 16 * 1024 * 1024


class ColdReceipt(ValueError):
    """Missing or invalid evidence requires cold production."""


def _unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ColdReceipt("duplicate_evidence_field")
        result[key] = value
    return result


def _constant(_value):
    raise ColdReceipt("json_constant")


def _finite(value):
    parsed = float(value)
    if not math.isfinite(parsed):
        raise ColdReceipt("json_nonfinite")
    return parsed


def strict_json(data):
    """One bounded duplicate-free finite JSON parser for every evidence owner."""
    if not isinstance(data, (bytes, bytearray, str)) or len(data) > MAX_EVIDENCE:
        raise ColdReceipt("evidence_byte_limit")
    try:
        return json.loads(data, object_pairs_hook=_unique, parse_constant=_constant,
                          parse_float=_finite)
    except (ValueError, UnicodeError, RecursionError) as error:
        raise ColdReceipt("evidence_malformed") from error


def _absolute(path):
    value = os.fspath(path)
    if not isinstance(value, str) or not value.startswith("/"):
        raise ColdReceipt("payload_absolute_root_required")
    components = value.split("/")[1:]
    if not components or any(part in ("", ".", "..") for part in components):
        raise ColdReceipt("payload_root_alias")
    if "\\" in value or "\0" in value:
        raise ColdReceipt("payload_root_alias")
    return components


def secure_read(path):
    components = _absolute(path)
    directory = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in components[:-1]:
            admitted = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                               dir_fd=directory)
            os.close(directory)
            directory = admitted
        descriptor = os.open(components[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                             dir_fd=directory)
    finally:
        os.close(directory)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1
                or metadata.st_size > MAX_EVIDENCE):
            raise ColdReceipt("bundle_not_regular")
        data = stream.read(MAX_EVIDENCE + 1)
        after = os.fstat(stream.fileno())
        if (after.st_size, after.st_mtime_ns, after.st_ctime_ns) != (
                metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns):
            raise ColdReceipt("bundle_changed")
    if len(data) > MAX_EVIDENCE:
        raise ColdReceipt("evidence_byte_limit")
    return data


