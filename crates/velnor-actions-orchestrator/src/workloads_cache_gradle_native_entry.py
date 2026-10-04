"""Shared validator for the one reviewed Gradle Java compile cache entry."""
import hashlib
import re
import zlib


EXPECTED_KEY = "3f89c0aec97a121579524bf681cfd366"
MAX_TAR_ENTRIES = 32
MAX_TAR_BYTES = 64 * 1024 * 1024
MAX_COMPRESSED_BYTES = MAX_TAR_BYTES + 64 * 1024
MAX_METADATA_BYTES = 64 * 1024
EXPECTED_TAR = {
    "METADATA": "file",
    "tree-destinationDirectory": "directory",
    "tree-destinationDirectory/com": "directory",
    "tree-destinationDirectory/com/chainargos": "directory",
    "tree-destinationDirectory/com/chainargos/processor": "directory",
    "tree-destinationDirectory/com/chainargos/processor/binding": "directory",
    "tree-destinationDirectory/com/chainargos/processor/binding/RpcEndpointBinding.class": "file",
    "tree-options.generatedSourceOutputDirectory": "directory",
    "tree-options.headerOutputDirectory": "directory",
    "tree-previousCompilationData": "file",
}
EXPECTED_FILE_SHA256 = {
    "tree-destinationDirectory/com/chainargos/processor/binding/RpcEndpointBinding.class": (
        15800,
        "f8784a67228df194a780bf5cb09d53f9d34cd93c174434df8b1b3c62bce73708",
    ),
    "tree-previousCompilationData": (
        742,
        "690aef7d0e0d070c7972ad90da721f26e7728133dd314b0f18fe47edf27c328e",
    ),
}
PAX_TIME = re.compile(rb"28 mtime=(\d{10})\.(\d{7})\n")
METADATA_TIME = re.compile(
    r"^#[A-Z][a-z]{2} [A-Z][a-z]{2} \d{2} \d{2}:\d{2}:\d{2} "
    r"(?:GMT[+-](?:0\d|1[0-4]):[0-5]\d|UTC|GMT) \d{4}$"
)


def gzip_payload(body):
    if len(body) > MAX_COMPRESSED_BYTES:
        raise ValueError("native cache gzip byte budget")
    if (len(body) < 10 or body[:3] != b"\x1f\x8b\x08" or body[3] != 0
            or body[4:8] != b"\0\0\0\0" or body[8] != 0 or body[9] != 255):
        raise ValueError("native cache gzip header")
    decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
    try:
        payload = decoder.decompress(body, MAX_COMPRESSED_BYTES + 1)
        if decoder.unconsumed_tail or len(payload) > MAX_COMPRESSED_BYTES:
            raise ValueError("native cache gzip payload budget")
        payload += decoder.flush()
    except zlib.error as error:
        raise ValueError("native cache gzip stream") from error
    if decoder.unused_data or not decoder.eof or len(payload) > MAX_COMPRESSED_BYTES:
        raise ValueError("native cache gzip stream")
    return payload


def octal_field(value):
    if (len(value) != 12 or value[-1:] != b" "
            or any(char not in b"01234567" for char in value[:-1])):
        raise ValueError("native cache tar numeric field")
    return int(value[:-1], 8)


def validate_header(header, name, size, seconds, kind, typeflag):
    expected_name = name.encode() + (b"/" if kind == "directory" else b"")
    expected_mode = b"0040755 " if kind == "directory" else b"0100644 "
    if len(expected_name) > 100 or len(header) != 512:
        raise ValueError("native cache tar header size")
    if (header[:100] != expected_name.ljust(100, b"\0")
            or header[100:108] != expected_mode
            or header[108:124] != b"0000000 " * 2
            or header[124:136] != f"{size:011o} ".encode()
            or header[136:148] != f"{seconds:011o} ".encode()
            or header[156:157] != typeflag
            or header[157:257] != bytes(100)
            or header[257:265] != b"ustar\x0000"
            or header[265:329] != bytes(64)
            or header[329:345] != b"0000000 " * 2
            or header[345:512] != bytes(167)):
        raise ValueError("native cache tar header fields")
    checksum = sum(header[:148]) + sum(b"        ") + sum(header[156:])
    if header[148:156] != f"{checksum:06o}\0 ".encode():
        raise ValueError("native cache tar checksum")


def read_pax(raw, offset, name, kind):
    header = raw[offset:offset + 512]
    record = raw[offset + 512:offset + 540]
    padding = raw[offset + 540:offset + 1024]
    match = PAX_TIME.fullmatch(record)
    if match is None or padding != bytes(484):
        raise ValueError("native cache pax record")
    seconds = int(match.group(1))
    logical_name = name + ("/" if kind == "directory" else "")
    pax_name = "./PaxHeaders.X/" + logical_name.replace("/", "_")
    validate_header(header, pax_name, 28, seconds, "file", b"x")
    return offset + 1024, seconds


def read_logical(raw, offset, name, kind, seconds):
    header = raw[offset:offset + 512]
    size = octal_field(header[124:136])
    if kind == "directory" and size != 0:
        raise ValueError("native cache directory size")
    if name == "METADATA" and size > MAX_METADATA_BYTES:
        raise ValueError("native cache metadata bound")
    expected = EXPECTED_FILE_SHA256.get(name)
    if expected is not None and size != expected[0]:
        raise ValueError("native cache output size")
    typeflag = b"5" if kind == "directory" else b"0"
    validate_header(header, name, size, seconds, kind, typeflag)
    data_start = offset + 512
    data_end = data_start + size
    padded_end = data_start + ((size + 511) // 512) * 512
    if padded_end > len(raw) or raw[data_end:padded_end] != bytes(padded_end - data_end):
        raise ValueError("native cache tar data padding")
    value = raw[data_start:data_end]
    if expected is not None and hashlib.sha256(value).hexdigest() != expected[1]:
        raise ValueError("native cache output digest")
    return padded_end, value


def tar_payload(body):
    payload = gzip_payload(body)
    if (len(payload) % 512 or len(EXPECTED_TAR) > MAX_TAR_ENTRIES
            or len(payload) < 1024):
        raise ValueError("native cache tar bounds")
    offset = 0
    metadata = None
    total = 0
    for name, kind in EXPECTED_TAR.items():
        offset, seconds = read_pax(payload, offset, name, kind)
        offset, value = read_logical(payload, offset, name, kind, seconds)
        total += len(value)
        if total > MAX_TAR_BYTES:
            raise ValueError("native cache tar byte budget")
        if name == "METADATA":
            metadata = value
    if metadata is None or payload[offset:] != bytes(1024):
        raise ValueError("native cache tar trailing bytes")
    return metadata


def validate_metadata(metadata):
    values = {}
    comments = []
    for line in metadata.decode("utf-8").splitlines():
        if line.startswith("#"):
            comments.append(line)
            continue
        if not line:
            continue
        key, separator, value = line.partition("=")
        if not separator or key in values:
            raise ValueError("native cache metadata shape")
        values[key] = value
    expected = {
        "buildCacheKey": EXPECTED_KEY,
        "gradleVersion": "9.5.1",
        "identity": r"\:processor-target-validation\:compileJava",
        "type": "org.gradle.api.internal.tasks.execution.TaskExecution",
    }
    if any(values.get(key) != value for key, value in expected.items()):
        raise ValueError("native cache metadata identity")
    if set(values) != set(expected) | {"buildInvocationId", "creationTime", "executionTime"}:
        raise ValueError("native cache metadata fields")
    if not re.fullmatch(r"[A-Za-z0-9]+", values["buildInvocationId"]):
        raise ValueError("native cache metadata invocation")
    if (not values["creationTime"].isdigit()
            or not values["executionTime"].isdigit()):
        raise ValueError("native cache metadata timing")
    if (len(comments) != 2 or comments[0] != "#Generated origin information"
            or not METADATA_TIME.fullmatch(comments[1])):
        raise ValueError("native cache metadata comments")


def validate_key_archive(body):
    validate_metadata(tar_payload(body))
