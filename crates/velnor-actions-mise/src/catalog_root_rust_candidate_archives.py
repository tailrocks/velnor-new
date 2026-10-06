"""Preflight and mirror the fixed official RootLinux Rust candidate archives."""

import hashlib
import io
import lzma
import os
import re
import ssl
import stat
import sys
import tarfile
import urllib.request


VERSION = "1.98.1"
TARGET = "x86_64-unknown-linux-gnu"
RELEASE_DATE = "2026-09-03"
MANIFEST_URL = "https://static.rust-lang.org/dist/channel-rust-1.98.1.toml"
ROOT_RELATIVE = "/velnor-control/root-rust-candidate"
COMPONENTS = ("rustc", "cargo", "rust-std", "clippy-preview", "rustfmt-preview")
_COMPONENT_FILES = {
    "rustc": "rustc-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
    "cargo": "cargo-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
    "rust-std": "rust-std-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
    "clippy-preview": "clippy-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
    "rustfmt-preview": "rustfmt-1.98.1-x86_64-unknown-linux-gnu.tar.xz",
}
_COMPONENT_FOLDERS = {
    "rustc": "rustc",
    "cargo": "cargo",
    "rust-std": "rust-std-x86_64-unknown-linux-gnu",
    "clippy-preview": "clippy-preview",
    "rustfmt-preview": "rustfmt-preview",
}
MAX_MANIFEST_BYTES = 32 * 1024 * 1024
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
MAX_MEMBER_BYTES = 512 * 1024 * 1024
MAX_TOTAL_BYTES = 2 * 1024 * 1024 * 1024
TIMEOUT_SECONDS = 30
_HEX = re.compile(r"[0-9a-f]{64}\Z")


def _require(condition, message):
    if not condition:
        raise ValueError(message)


def _digest(value, label):
    _require(type(value) is str and _HEX.fullmatch(value) is not None,
             label + " must be lowercase SHA-256")


def _relative(value, label):
    _require(type(value) is str and value.isascii() and value, label + " is invalid")
    _require("\\" not in value and "\0" not in value, label + " is invalid")
    parts = value.split("/")
    _require(all(part not in ("", ".", "..") for part in parts), label + " is invalid")
    _require(all(ord(char) >= 32 and ord(char) != 127 for char in value),
             label + " contains control characters")
    return value


def _validate_config(config):
    _require(type(config) is dict and set(config) == {"manifest", "payload"},
             "candidate configuration shape")
    manifest = config["manifest"]
    keys = {"version", "target", "manifest_url", "manifest_sha256", "components"}
    _require(type(manifest) is dict and set(manifest) == keys, "candidate manifest shape")
    _require(manifest["version"] == VERSION and manifest["target"] == TARGET,
             "candidate manifest pin")
    _require(manifest["manifest_url"] == MANIFEST_URL, "candidate manifest URL")
    _digest(manifest["manifest_sha256"], "candidate manifest digest")
    components = manifest["components"]
    _require(type(components) is list and len(components) == len(COMPONENTS),
             "candidate component count")
    records = {name: [] for name in COMPONENTS}
    for component, item in zip(COMPONENTS, components):
        _require(type(item) is dict and set(item) == {"component", "xz_url", "xz_sha256"},
                 "candidate component shape")
        _require(item["component"] == component, "candidate component identity")
        expected_url = "https://static.rust-lang.org/dist/" + RELEASE_DATE + "/" \
            + _COMPONENT_FILES[component]
        _require(item["xz_url"] == expected_url, "candidate component URL")
        _digest(item["xz_sha256"], "candidate archive digest")
    payload = config["payload"]
    _require(type(payload) is list and payload, "candidate payload shape")
    paths, members = set(), set()
    for item in payload:
        fields = {"path", "component", "archive_member", "size", "sha256", "mode"}
        _require(type(item) is dict and set(item) == fields, "candidate payload record")
        component = item["component"]
        _require(component in records, "candidate payload component")
        path = _relative(item["path"], "candidate installed path")
        member = _relative(item["archive_member"], "candidate archive member")
        _require(type(item["size"]) is int and not isinstance(item["size"], bool)
                 and 0 <= item["size"] <= MAX_MEMBER_BYTES, "candidate payload size")
        _digest(item["sha256"], "candidate payload digest")
        _require(type(item["mode"]) is int and not isinstance(item["mode"], bool)
                 and item["mode"] in (0o644, 0o755), "candidate payload mode")
        archive_root = _COMPONENT_FILES[component][:-len(".tar.xz")]
        folder = _COMPONENT_FOLDERS[component]
        expected_member = archive_root + "/" + folder + "/" + path
        _require(member == expected_member, "candidate archive mapping")
        _require(path not in paths and member not in members, "candidate payload duplicate")
        paths.add(path)
        members.add(member)
        records[component].append(item)
    _require(all(records.values()), "candidate component payload incomplete")
    return manifest, records


class _RejectRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, newurl):
        raise ValueError("official Rust source redirect rejected")


def _tls_context():
    cafiles = {"linux": "/etc/ssl/certs/ca-certificates.crt",
               "darwin": "/etc/ssl/cert.pem"}
    cafile = cafiles.get(sys.platform)
    _require(cafile is not None, "official Rust source TLS platform")
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.load_verify_locations(cafile=cafile)
    return context


def _read_response(response, limit):
    chunks, total = [], 0
    while True:
        chunk = response.read(min(1024 * 1024, limit + 1 - total))
        if not chunk:
            return b"".join(chunks)
        total += len(chunk)
        _require(total <= limit, "official Rust source size limit")
        chunks.append(chunk)


def _download_bytes(url, expected, limit):
    official = {MANIFEST_URL}
    official.update("https://static.rust-lang.org/dist/" + RELEASE_DATE + "/" + name
                    for name in _COMPONENT_FILES.values())
    _require(url in official, "official Rust source URL")
    _digest(expected, "official Rust source digest")
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=_tls_context()),
        _RejectRedirect(),
    )
    request = urllib.request.Request(url, headers={"Accept-Encoding": "identity"})
    with opener.open(request, timeout=TIMEOUT_SECONDS) as response:
        _require(response.status == 200, "official Rust source HTTP 200 required")
        _require(response.geturl() == url, "official Rust source final URL changed")
        data = _read_response(response, limit)
    _require(hashlib.sha256(data).hexdigest() == expected,
             "official Rust source SHA-256 mismatch")
    return data


def _member_bytes(tar, member):
    stream = tar.extractfile(member)
    _require(stream is not None, "Rust component regular member unreadable")
    with stream:
        data = stream.read(member.size + 1)
    _require(len(data) == member.size, "Rust component member size mismatch")
    return data


def _manifest_entries(data):
    _require(data and len(data) <= MAX_MANIFEST_BYTES and data.endswith(b"\n"),
             "Rust component manifest.in shape")
    entries, seen = [], set()
    for line in data[:-1].split(b"\n"):
        _require(line.startswith(b"file:"), "Rust component manifest.in is not FILE-only")
        try:
            path = line[5:].decode("ascii")
        except UnicodeDecodeError as error:
            raise ValueError("Rust component manifest.in path encoding") from error
        _relative(path, "Rust component manifest.in path")
        _require(path not in seen, "Rust component manifest.in duplicate")
        seen.add(path)
        entries.append(path)
    _require(entries, "Rust component manifest.in empty")
    return entries


def _validate_archive(data, component, records):
    root = _COMPONENT_FILES[component][:-len(".tar.xz")]
    folder = _COMPONENT_FOLDERS[component]
    prefix = root + "/" + folder
    manifest_name = prefix + "/manifest.in"
    expected = {item["archive_member"]: item for item in records}
    _require(len(expected) == len(records), "Rust component payload duplicate")
    _require(type(data) is bytes and 0 < len(data) <= MAX_ARCHIVE_BYTES,
             "Rust component archive size")
    regular, directories, seen = set(), set(), set()
    manifest_data = None
    total = 0
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:xz", errorlevel=2) as tar:
            for count, member in enumerate(tar, start=1):
                _require(count <= 4096, "Rust component archive member count")
                name = _relative(member.name, "Rust component member path")
                _require(name not in seen, "Rust component archive duplicate")
                seen.add(name)
                if member.type == tarfile.DIRTYPE:
                    _require(member.size == 0, "Rust component directory size")
                    directories.add(name)
                    continue
                _require(member.type in (tarfile.REGTYPE, tarfile.AREGTYPE)
                         and member.sparse is None,
                         "Rust component archive member type")
                _require(0 <= member.size <= MAX_MEMBER_BYTES,
                         "Rust component member size")
                total += member.size
                _require(total <= MAX_TOTAL_BYTES, "Rust component archive content size")
                _require(name.startswith(root + "/"),
                         "Rust component file escapes component")
                payload = _member_bytes(tar, member)
                regular.add(name)
                if name == manifest_name:
                    manifest_data = payload
                    continue
                if name not in expected:
                    continue
                record = expected[name]
                normalized = ((member.mode & 0o700) | ((member.mode & 0o500) >> 3) |
                              ((member.mode & 0o500) >> 6)) & ~0o022
                _require(normalized == record["mode"],
                         "Rust component tar mode mismatch")
                _require(len(payload) == record["size"] and
                         hashlib.sha256(payload).hexdigest() == record["sha256"],
                         "Rust component payload bytes mismatch")
    except (EOFError, OSError, tarfile.TarError, lzma.LZMAError) as error:
        raise ValueError("Rust component archive is malformed") from error
    _require(manifest_data is not None and set(expected) | {manifest_name} <= regular,
             "Rust component archive file set")
    _require(root in directories and prefix in directories, "Rust component root directories")
    for directory in directories:
        _require(directory in (root, prefix) or directory.startswith(prefix + "/"),
                 "Rust component directory escapes component")
    entries = _manifest_entries(manifest_data)
    expected_paths = {record["path"] for record in records}
    _require(set(entries) == expected_paths and len(entries) == len(expected_paths),
             "Rust component manifest.in entries")


def _owned_directory(path, label, require_owner=True):
    try:
        info = os.lstat(path)
    except FileNotFoundError as error:
        raise ValueError(label + " missing") from error
    _require(stat.S_ISDIR(info.st_mode) and not stat.S_ISLNK(info.st_mode),
             label + " must be a directory")
    if require_owner:
        _require(info.st_uid == os.geteuid() and not info.st_mode & 0o022,
                 label + " ownership")


def _safe_root():
    temp = os.environ.get("RUNNER_TEMP")
    advertised = os.environ.get("VELNOR_ROOT_RUST_CANDIDATE_ROOT")
    _require(type(temp) is str and temp.isascii() and temp != "/" and temp.startswith("/") and
             all(ord(char) >= 32 and ord(char) != 127 for char in temp) and
             all(part not in ("", ".", "..") for part in temp.split("/")[1:]),
             "RUNNER_TEMP binding")
    expected = temp + ROOT_RELATIVE
    _require(advertised == expected, "Root Rust candidate root binding")
    current = "/"
    for part in expected.split("/")[1:]:
        current += part if current == "/" else "/" + part
        owned = current == temp or current.startswith(temp + "/")
        _owned_directory(current, "Root Rust candidate parent", owned)
    native = expected + "/native-dist"
    try:
        os.lstat(native)
    except FileNotFoundError:
        return expected, native
    raise ValueError("Root Rust candidate native-dist must be fresh")


def _mkdir_fresh(path):
    try:
        os.mkdir(path, 0o700)
    except FileExistsError as error:
        raise ValueError("Root Rust candidate output already exists") from error
    _owned_directory(path, "Root Rust candidate output")


def _write_new(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        view = memoryview(data)
        while view:
            count = os.write(descriptor, view)
            _require(count > 0, "Root Rust candidate short write")
            view = view[count:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    info = os.lstat(path)
    _require(stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode)
             and info.st_uid == os.geteuid() and info.st_nlink == 1,
             "Root Rust candidate output file shape")


def _publish(root, manifest_data, archives):
    native = root + "/native-dist"
    _mkdir_fresh(native)
    dist = native + "/dist"
    _mkdir_fresh(dist)
    dated = dist + "/" + RELEASE_DATE
    _mkdir_fresh(dated)
    _write_new(dist + "/channel-rust-1.98.1.toml", manifest_data)
    _write_new(dist + "/channel-rust-1.98.1.toml.sha256",
               (hashlib.sha256(manifest_data).hexdigest() +
                "  channel-rust-1.98.1.toml\n").encode("ascii"))
    for component in COMPONENTS:
        filename, data = archives[component]
        _write_new(dated + "/" + filename, data)


def _preflight(config, fetch=None):
    if fetch is None:
        fetch = _download_bytes
    manifest, records = _validate_config(config)
    manifest_data = fetch(manifest["manifest_url"], manifest["manifest_sha256"],
                          MAX_MANIFEST_BYTES)
    _require(hashlib.sha256(manifest_data).hexdigest() == manifest["manifest_sha256"],
             "candidate manifest SHA-256 mismatch")
    archives = {}
    for component in COMPONENTS:
        item = next(entry for entry in manifest["components"] if entry["component"] == component)
        data = fetch(item["xz_url"], item["xz_sha256"], MAX_ARCHIVE_BYTES)
        _require(hashlib.sha256(data).hexdigest() == item["xz_sha256"],
                 "candidate archive SHA-256 mismatch")
        _validate_archive(data, component, records[component])
        archives[component] = (_COMPONENT_FILES[component], data)
    return manifest_data, archives


def _acquire(config, fetch=None):
    if fetch is None:
        fetch = _download_bytes
    root, _ = _safe_root()
    manifest_data, archives = _preflight(config, fetch)
    _publish(root, manifest_data, archives)


def candidate_acquire_archives():
    """Fetch and mirror the compiled CONFIG; no caller-supplied configuration is accepted."""
    config = globals().get("CONFIG")
    _require(config is not None, "compiled Root Rust candidate CONFIG missing")
    _acquire(config)
