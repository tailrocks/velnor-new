"""Bounded crates.io sparse-index and checksum-bound baseline acquisition."""
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import ssl
import tarfile
import tempfile
import time
import tomllib
from typing import TypedDict
import urllib.error
import urllib.request
import zlib


REGISTRY_INDEX_LIMIT = 16 * 1024 * 1024
REGISTRY_ARCHIVE_LIMIT = 32 * 1024 * 1024
REGISTRY_TAR_LIMIT = 128 * 1024 * 1024
REGISTRY_FILE_LIMIT = 32 * 1024 * 1024
REGISTRY_MEMBER_LIMIT = 10000
REGISTRY_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}")



class RegistryBaseline(TypedDict):
    registry: str
    name: str
    version: str
    checksum: str
    yanked: bool
    package_root: str
    index_url: str
    index_sha256: str
    archive_url: str
    archive_sha256: str
    archive_path: str
    manifest_sha256: str
    inventory_sha256: str
    index_version_path: str
    index_version_sha256: str


class RegistryNotFound(ReconcileError):
    """Fixed registry 404; absence alone never authorizes an initial release."""

    def __init__(self, url):
        super().__init__("registry_not_found")
        self.url = url


class RegistryNoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        raise ReconcileError("registry_redirect")


def registry_fetch(url, max_bytes):
    """Fixed HTTPS destinations; no proxy, credentials, redirects or ambient headers."""
    require(re.fullmatch(
        r"https://(?:index\.crates\.io/[a-z0-9_/-]+|"
        r"static\.crates\.io/crates/[A-Za-z0-9_-]+/"
        r"[A-Za-z0-9_.+-]+\.crate)", url), "registry_fetch_url")
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    paths = ssl.get_default_verify_paths()
    context.load_verify_locations(cafile=paths.openssl_cafile, capath=paths.openssl_capath)
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}), RegistryNoRedirect(),
        urllib.request.HTTPSHandler(context=context))
    deadline = time.monotonic() + 30
    try:
        with opener.open(urllib.request.Request(url, headers={"Accept": "application/octet-stream"}),
                         timeout=10) as response:
            require(response.status == 200 and response.geturl() == url, "registry_response")
            length = response.headers.get("Content-Length")
            require(length is None or (length.isdecimal() and int(length) <= max_bytes),
                    "registry_response_size")
            chunks, size = [], 0
            while True:
                require(time.monotonic() < deadline, "registry_timeout")
                chunk = response.read(min(65536, max_bytes + 1 - size))
                if not chunk:
                    return b"".join(chunks)
                size += len(chunk)
                require(size <= max_bytes, "registry_response_size")
                chunks.append(chunk)
    except urllib.error.HTTPError as error:
        error.close()
        if error.code == 404:
            raise RegistryNotFound(url) from error
        raise ReconcileError("registry_fetch_failed") from error
    except (OSError, urllib.error.URLError) as error:
        raise ReconcileError("registry_fetch_failed") from error


def registry_index_path(name):
    name = name.lower()
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def registry_unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "registry_duplicate_json_key")
        result[key] = value
    return result


def registry_index_record(raw, name, version):
    require(isinstance(raw, bytes) and len(raw) <= REGISTRY_INDEX_LIMIT, "registry_index_size")
    matches = []
    try:
        for line in raw.decode("utf-8").splitlines():
            entry = json.loads(line, object_pairs_hook=registry_unique_object,
                               parse_constant=lambda _: require(False, "registry_json_constant"))
            require(isinstance(entry, dict), "registry_index_entry")
            canonical = entry.get("name")
            require(isinstance(canonical, str) and REGISTRY_NAME.fullmatch(canonical) and
                    canonical.lower().replace("_", "-") == name.lower().replace("_", "-"),
                    "registry_index_name")
            if entry.get("vers") == version:
                require(canonical == name, "registry_canonical_name")
                require(isinstance(entry.get("cksum"), str) and
                        re.fullmatch(r"[0-9a-f]{64}", entry["cksum"]) and
                        type(entry.get("yanked")) is bool, "registry_index_identity")
                matches.append((entry, line.encode("utf-8")))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ReconcileError("registry_index_decode") from error
    require(len(matches) == 1, "registry_version_missing_or_duplicate")
    return matches[0]


def registry_index_entry(raw, name, version):
    return registry_index_record(raw, name, version)[0]


def registry_tar_bytes(raw):
    require(isinstance(raw, bytes) and len(raw) <= REGISTRY_ARCHIVE_LIMIT, "registry_archive_size")
    try:
        decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
        unpacked = decoder.decompress(raw, REGISTRY_TAR_LIMIT + 1)
        require(len(unpacked) <= REGISTRY_TAR_LIMIT and decoder.eof and
                not decoder.unused_data and not decoder.unconsumed_tail, "registry_tar_size")
        return unpacked
    except zlib.error as error:
        raise ReconcileError("registry_archive_gzip") from error


def registry_extract(raw, staging, prefix):
    seen, files, total = set(), 0, 0
    directories = {}
    try:
        with tarfile.open(fileobj=io.BytesIO(registry_tar_bytes(raw)), mode="r:") as archive:
            for member in archive:
                parts = member.name.split("/")
                if member.isdir() and parts[-1] == "":
                    parts.pop()
                require(parts and parts[0] == prefix and
                        all(part not in ("", ".", "..") and "\\" not in part and
                            "\x00" not in part for part in parts), "registry_archive_path")
                path = "/".join(parts)
                require(path not in seen and len(seen) < REGISTRY_MEMBER_LIMIT,
                        "registry_archive_duplicate_or_count")
                seen.add(path)
                require(member.isfile() or member.isdir(), "registry_archive_type")
                require(set(member.pax_headers) <= {"path", "size", "mtime", "atime",
                        "ctime", "uid", "gid", "uname", "gname", "comment", "charset"} and
                        member.sparse is None, "registry_archive_extensions")
                require(member.mode & ~0o777 == 0, "registry_archive_special_mode")
                target = staging.joinpath(*parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                    directories[target] = member.mode
                    continue
                total += member.size
                require(len(parts) > 1 and 0 <= member.size <= REGISTRY_FILE_LIMIT and
                        total <= REGISTRY_TAR_LIMIT, "registry_archive_file_size")
                target.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                require(source is not None, "registry_archive_file")
                with source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, 65536)
                target.chmod(member.mode)
                files += 1
        # Native tar applies explicit directory metadata after writing children.
        # Implicit parents retain mkdir's ordinary permissions under the umask.
        for target in sorted(directories, key=lambda path: len(path.parts), reverse=True):
            target.chmod(directories[target])
    except (tarfile.TarError, OSError) as error:
        raise ReconcileError("registry_archive_extract") from error
    require(files > 0, "registry_archive_empty")


def registry_remove_staging(staging):
    """Restore traversal only for abandoned private extraction before removal."""
    directories = [staging]
    for directory in directories:
        directory.chmod(0o700)
        directories.extend(path for path in directory.iterdir() if path.is_dir())
    shutil.rmtree(staging)


def registry_bind_manifest(root, name, version):
    manifest = root / "Cargo.toml"
    require(manifest.is_file() and manifest.stat().st_size <= 1024 * 1024,
            "registry_manifest_missing_or_size")
    try:
        package = tomllib.loads(manifest.read_text(encoding="utf-8")).get("package")
    except (UnicodeError, tomllib.TOMLDecodeError) as error:
        raise ReconcileError("registry_manifest_decode") from error
    require(isinstance(package, dict) and package.get("name") == name and
            package.get("version") == version, "registry_manifest_identity")


def registry_inventory(root):
    return {path.relative_to(root).as_posix(): {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "mode": "100755" if path.stat().st_mode & 0o111 else "100644"}
        for path in sorted(root.rglob("*")) if path.is_file()}


def acquire_registry_baseline(name, version, destination, fetch=None) -> RegistryBaseline:
    """Return proof only after index checksum, closed archive and manifest binding."""
    require(isinstance(name, str) and REGISTRY_NAME.fullmatch(name), "registry_name")
    _require_version(version)
    fetch = registry_fetch if fetch is None else fetch
    index_url = "https://index.crates.io/" + registry_index_path(name)
    archive_url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
    index_raw = fetch(index_url, REGISTRY_INDEX_LIMIT)
    entry, index_version_raw = registry_index_record(index_raw, name, version)
    raw = fetch(archive_url, REGISTRY_ARCHIVE_LIMIT)
    require(isinstance(raw, bytes) and len(raw) <= REGISTRY_ARCHIVE_LIMIT and
            hashlib.sha256(raw).hexdigest() == entry["cksum"], "registry_archive_checksum")
    destination = Path(destination).absolute()
    require(not destination.exists() and not destination.is_symlink(), "registry_destination_exists")
    require(all(not parent.is_symlink() for parent in destination.parents), "registry_destination_symlink")
    destination.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".registry-baseline-", dir=destination.parent))
    prefix = f"{name}-{version}"
    try:
        registry_extract(raw, staging, prefix)
        registry_bind_manifest(staging / prefix, name, version)
        with (staging / "archive.crate").open("xb") as stream:
            stream.write(raw)
        (staging / "index-version.json").write_bytes(index_version_raw)
        inventory = registry_inventory(staging / prefix)
        manifest_sha = inventory["Cargo.toml"]["sha256"]
        inventory_sha = hashlib.sha256(json.dumps(
            inventory, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        staging.rename(destination)
    finally:
        if staging.exists():
            registry_remove_staging(staging)
    return {"registry": "crates-io", "name": name, "version": version,
            "checksum": entry["cksum"], "yanked": entry["yanked"],
            "package_root": str(destination / prefix), "index_url": index_url,
            "index_sha256": hashlib.sha256(index_raw).hexdigest(),
            "archive_url": archive_url, "archive_sha256": entry["cksum"],
            "archive_path": str(destination / "archive.crate"),
            "manifest_sha256": manifest_sha, "inventory_sha256": inventory_sha,
            "index_version_path": str(destination / "index-version.json"),
            "index_version_sha256": hashlib.sha256(index_version_raw).hexdigest()}
