"""Cargo archive identity and fixed crates.io transport primitives."""
import gzip
import hashlib
import io
import json
from pathlib import PurePosixPath
import re
import tarfile
import time
import tomllib
import unicodedata
import urllib.error
import urllib.request


SOURCE_INTENT_MAX_PATH_BYTES = 4095
SOURCE_INTENT_MAX_NAMESPACE_NODES = 40000


def normalized_toml(content):
    def typed(value):
        kind = type(value).__name__
        if isinstance(value, dict):
            return [kind, [[key, typed(item)] for key, item in sorted(value.items())]]
        if isinstance(value, list):
            return [kind, [typed(item) for item in value]]
        # Type tags distinguish TOML date/time scalars from identical quoted text.
        return [kind, str(value)]
    return json.dumps(typed(tomllib.loads(content.decode("utf-8"))),
                      separators=(",", ":")).encode("utf-8")


def _source_intent_namespace(parts, namespace, budget):
    for index, component in enumerate(parts):
        key = unicodedata.normalize("NFC", unicodedata.normalize("NFC", component).casefold())
        if key not in namespace:
            require(budget[0] < SOURCE_INTENT_MAX_NAMESPACE_NODES, "archive_namespace_size")
            budget[0] += 1
            namespace[key] = {"name": component, "children": {}, "file": False}
        node = namespace[key]
        require(node["name"] == component, "archive_namespace_alias")
        terminal = index == len(parts) - 1
        if terminal:
            require(not node["file"] and not node["children"], "archive_namespace_topology")
            node["file"] = True
        else:
            require(not node["file"], "archive_namespace_topology")
        namespace = node["children"]


def source_intent_content_inventory(data, name, version):
    """Inventory content for caller-supplied Cargo-validated package identities.

    Checks enforce safe components and exact identity, not Cargo's SemVer
    grammar. Authenticated source intent owns provenance independently.
    """
    require(type(data) is bytes and len(data) <= 64 * 1024 * 1024, "archive_size")
    require(isinstance(name, str) and
            re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", name) and
            isinstance(version, str) and re.fullmatch(r"[0-9A-Za-z.+-]+", version),
            "archive_package_identity")
    files = {}
    namespace = {}
    namespace_budget = [0]
    manifest = None
    prefix = f"{name}-{version}/"
    total = 0
    with gzip.GzipFile(fileobj=io.BytesIO(data)) as compressed:
        expanded = compressed.read(128 * 1024 * 1024 + 1)
        require(len(expanded) <= 128 * 1024 * 1024, "archive_total_expansion")
    with tarfile.open(fileobj=io.BytesIO(expanded), mode="r:") as archive:
        for member in archive:
            require(member.name.startswith(prefix), "archive_prefix")
            relative = member.name[len(prefix):]
            require(not any(ord(character) < 32 or ord(character) == 127 or
                            0xd800 <= ord(character) <= 0xdfff for character in relative) and
                    len(relative.encode("utf-8")) <= SOURCE_INTENT_MAX_PATH_BYTES,
                    "archive_path_size_or_characters")
            path = PurePosixPath(relative)
            require(relative and not path.is_absolute() and ".." not in path.parts and
                    str(path) == relative and "\\" not in relative, "archive_path")
            require(member.isfile(), "archive_nonregular")
            require(relative not in files and 0 <= member.size <= 16 * 1024 * 1024,
                    "archive_member")
            require(all(len(component.encode("utf-8")) <= 255 for component in path.parts),
                    "archive_component_size")
            _source_intent_namespace(path.parts, namespace, namespace_budget)
            total += member.size
            require(total <= 128 * 1024 * 1024 and len(files) < 20000, "archive_expanded_size")
            stream = archive.extractfile(member)
            require(stream is not None, "archive_stream")
            content = stream.read(member.size + 1)
            require(len(content) == member.size, "archive_truncated")
            if relative == "Cargo.toml":
                manifest = tomllib.loads(content.decode("utf-8"))
            normalized = normalized_toml(content) if relative in ("Cargo.toml", "Cargo.lock") else content
            files[relative] = {"sha256": hashlib.sha256(normalized).hexdigest(), "size": len(normalized)}
    package = manifest.get("package") if isinstance(manifest, dict) else None
    require(isinstance(package, dict) and package.get("name") == name and
            package.get("version") == version, "archive_identity")
    feature_map = manifest.get("features", {})
    require(isinstance(feature_map, dict) and
            all(isinstance(key, str) and isinstance(values, list) and
                all(isinstance(value, str) for value in values)
                for key, values in feature_map.items()), "archive_features")
    return {"files": dict(sorted(files.items())), "features": feature_map}


def inventory(data, name, version, source_sha):
    require(len(data) <= 64 * 1024 * 1024, "archive_size")
    files = {}
    vcs = None
    manifest = None
    prefix = f"{name}-{version}/"
    total = 0
    with gzip.GzipFile(fileobj=io.BytesIO(data)) as compressed:
        expanded = compressed.read(128 * 1024 * 1024 + 1)
        require(len(expanded) <= 128 * 1024 * 1024, "archive_total_expansion")
    with tarfile.open(fileobj=io.BytesIO(expanded), mode="r:") as archive:
        for member in archive:
            require(member.name.startswith(prefix), "archive_prefix")
            relative = member.name[len(prefix):]
            path = PurePosixPath(relative)
            require(relative and not path.is_absolute() and ".." not in path.parts and
                    str(path) == relative and "\\" not in relative, "archive_path")
            require(member.isfile(), "archive_nonregular")
            require(relative not in files and member.size <= 16 * 1024 * 1024, "archive_member")
            total += member.size
            require(total <= 128 * 1024 * 1024 and len(files) < 20000, "archive_expanded_size")
            stream = archive.extractfile(member)
            require(stream is not None, "archive_stream")
            content = stream.read(member.size + 1)
            require(len(content) == member.size, "archive_truncated")
            if relative == ".cargo_vcs_info.json":
                vcs = decode_json(content)
            if relative == "Cargo.toml":
                manifest = tomllib.loads(content.decode("utf-8"))
            normalized = content
            if relative in ("Cargo.toml", "Cargo.lock"):
                normalized = normalized_toml(content)
            if relative == ".cargo_vcs_info.json":
                require(set(vcs) == {"git", "path_in_vcs"} and isinstance(vcs["path_in_vcs"], str),
                        "archive_vcs_shape")
                normalized = json.dumps({"path_in_vcs": vcs["path_in_vcs"]}, sort_keys=True).encode("utf-8")
            files[relative] = {"sha256": hashlib.sha256(normalized).hexdigest(), "size": len(normalized)}
    require(vcs is not None and vcs.get("git", {}).get("sha1") == source_sha and
            vcs.get("git", {}).get("dirty", False) is False, "archive_source_provenance")
    require(manifest is not None and manifest.get("package", {}).get("name") == name and
            manifest.get("package", {}).get("version") == version, "archive_identity")
    # Ignore tar/compression headers, TOML formatting, and validated VCS SHA/dirty only.
    return {"files": dict(sorted(files.items())), "features": manifest.get("features", {})}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        raise ReconcileError("registry_redirect")


def fetch(url, limit=64 * 1024 * 1024):
    require(url.startswith(("https://crates.io/api/v1/crates/", "https://index.crates.io/",
                            "https://static.crates.io/crates/")), "registry_origin")
    request = urllib.request.Request(url, headers={"User-Agent": "velnor-release-reconciliation/1",
                                                 "Cache-Control": "no-cache"})
    opener = urllib.request.build_opener(NoRedirect())
    for attempt, delay in enumerate((0, 1, 2, 4, 8)):
        if delay:
            time.sleep(delay)
        try:
            with opener.open(request, timeout=20) as response:
                data = response.read(limit + 1)
                require(len(data) <= limit, "registry_response_size")
                return data
        except urllib.error.HTTPError as error:
            if error.code == 404 and attempt == 4:
                return None
            require(error.code in (404, 429, 500, 502, 503, 504), f"registry_http:{error.code}")
        except (urllib.error.URLError, TimeoutError):
            if attempt == 4:
                raise ReconcileError("registry_unavailable")
    raise ReconcileError("registry_unavailable")
