"""Keep only the verified Gradle JARs in the transferable files-2.1 store."""
import hashlib
import json
import os
from pathlib import Path
import stat


CLOSED = {
    ("io.micronaut", "micronaut-core", "4.10.14"): "2485a578736b3d013aecf17d5c1a4ee2669754af185d5bc6b5b12c7d685129ad",
    ("org.slf4j", "slf4j-api", "2.0.17"): "7b751d952061954d5abfed7181c1f645d336091b679891591d63329c622eb832",
}
MAX_MANIFEST = 1024 * 1024
MAX_JAR = 64 * 1024 * 1024
MAX_ENTRIES = 4096
MAX_TOTAL = 128 * 1024 * 1024
HOME_ROLES = {
    "consumer": ("velnor", "native", "gradle"),
    "producer": ("velnor", "native", "gradle-producer", "gradle-home"),
}


def reject_links(path):
    if not path.is_absolute() or any(part in (".", "..") for part in path.parts):
        raise ValueError("unresolved Gradle path")
    current = Path(path.anchor)
    for component in path.parts[1:]:
        current /= component
        if current.is_symlink():
            raise ValueError("redirected Gradle path")


def home():
    value = os.environ.get("GRADLE_USER_HOME", "")
    runner_value = os.environ.get("RUNNER_TEMP", "")
    role = os.environ.get("VELNOR_GRADLE_ARTIFACT_HOME_ROLE", "")
    components = HOME_ROLES.get(role)
    if components is None:
        raise ValueError("invalid Gradle artifact home role")
    root = Path(value)
    runner = Path(runner_value)
    expected = runner.joinpath(*components)
    reject_links(runner)
    reject_links(root)
    if not runner.is_dir() or root != expected:
        raise ValueError("invalid Gradle user home")
    current = runner
    for component in components:
        child = current / component
        if child.is_symlink() or not child.is_dir():
            raise ValueError("redirected Gradle user home")
        current = child
    return current.resolve()


def cache_root(root):
    current = root
    for component in ("caches", "modules-2", "files-2.1"):
        current = current / component
        if current.is_symlink() or not current.is_dir():
            raise ValueError("redirected Gradle artifact root")
    return current


def read_manifest(root):
    target = root / "velnor-public-artifacts.json"
    if target.is_symlink() or not target.is_file() or os.stat(target, follow_symlinks=False).st_nlink != 1:
        raise ValueError("missing or redirected artifact manifest")
    if os.stat(target, follow_symlinks=False).st_size > MAX_MANIFEST:
        raise ValueError("artifact manifest exceeds proof bound")
    values = json.loads(target.read_text(encoding="utf-8"))
    if not isinstance(values, list) or len(values) != len(CLOSED):
        raise ValueError("invalid artifact manifest")
    expected_keys = {"group", "module", "version", "sha256", "path"}
    seen = set()
    entries = []
    artifacts = cache_root(root)
    for value in values:
        if not isinstance(value, dict) or set(value) != expected_keys:
            raise ValueError("invalid artifact manifest entry")
        key = (value["group"], value["module"], value["version"])
        if key in seen or key not in CLOSED or value["sha256"] != CLOSED[key]:
            raise ValueError("unapproved artifact manifest entry")
        path = Path(value["path"])
        if not path.is_absolute():
            raise ValueError("artifact path is not absolute")
        relative = path.relative_to(artifacts)
        parts = relative.parts
        if len(parts) != 5 or parts[:3] != key or parts[4] != key[1] + "-" + key[2] + ".jar":
            raise ValueError("artifact path identity mismatch")
        if len(parts[3]) != 40 or any(byte not in "0123456789abcdef" for byte in parts[3]):
            raise ValueError("artifact path digest invalid")
        current = artifacts
        for component in parts[:-1]:
            current = current / component
            if current.is_symlink() or not current.is_dir():
                raise ValueError("redirected artifact directory")
        if path.is_symlink() or not path.is_file() or os.stat(path, follow_symlinks=False).st_nlink != 1:
            raise ValueError("redirected artifact path")
        info = os.stat(path, follow_symlinks=False)
        if info.st_size > MAX_JAR:
            raise ValueError("artifact exceeds proof bound")
        digest = hashlib.sha256()
        checksum = hashlib.sha1()
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
                checksum.update(chunk)
        if digest.hexdigest() != value["sha256"]:
            raise ValueError("artifact fingerprint mismatch")
        if checksum.hexdigest() != parts[3]:
            raise ValueError("artifact path digest mismatch")
        seen.add(key)
        entries.append((path, Path(*parts), value["sha256"]))
    return artifacts, entries


def sanitize():
    if os.environ.get("VELNOR_GRADLE_PUBLIC_PROOF_SAFE") != "true":
        raise ValueError("public artifact proof missing")
    root = home()
    artifacts, entries = read_manifest(root)
    allowed = {relative.as_posix() for _, relative, _ in entries}
    expected = {relative.as_posix(): digest for _, relative, digest in entries}
    allowed_dirs = set()
    for relative in allowed:
        allowed_dirs.update(str(parent) for parent in Path(relative).parents if str(parent) != ".")
    flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    owned_fd = os.open(artifacts, flags)
    entries_seen = 0
    total_bytes = 0
    try:
        for directory, dirs, files, current_fd in os.fwalk(".", dir_fd=owned_fd, follow_symlinks=False):
            if entries_seen + len(dirs) + len(files) > MAX_ENTRIES:
                raise ValueError("artifact cache entry bound exceeded")
            entries_seen += len(dirs) + len(files)
            for name in dirs:
                info = os.stat(name, dir_fd=current_fd, follow_symlinks=False)
                if not stat.S_ISDIR(info.st_mode):
                    raise ValueError("redirected artifact directory")
            for name in files:
                info = os.stat(name, dir_fd=current_fd, follow_symlinks=False)
                if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                    raise ValueError("redirected artifact file")
                total_bytes += info.st_size
                if total_bytes > MAX_TOTAL:
                    raise ValueError("artifact cache byte bound exceeded")
                relative = (Path(directory) / name).as_posix()
                if relative not in allowed:
                    os.unlink(name, dir_fd=current_fd)
                    continue
                source_fd = os.open(name, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=current_fd)
                try:
                    info = os.fstat(source_fd)
                    if info.st_nlink != 1 or info.st_size > MAX_JAR:
                        raise ValueError("redirected opened artifact")
                    digest = hashlib.sha256()
                    source = os.fdopen(source_fd, "rb")
                    source_fd = None
                    size = 0
                    with source:
                        for chunk in iter(lambda: source.read(1024 * 1024), b""):
                            size += len(chunk)
                            if size > MAX_JAR:
                                raise ValueError("artifact exceeds proof bound")
                            digest.update(chunk)
                    if digest.hexdigest() != expected[relative]:
                        raise ValueError("artifact changed during sanitize")
                finally:
                    if source_fd is not None:
                        os.close(source_fd)
        for directory, dirs, _, current_fd in os.fwalk(".", topdown=False, dir_fd=owned_fd, follow_symlinks=False):
            for name in dirs:
                relative = (Path(directory) / name).as_posix()
                if relative not in allowed_dirs:
                    os.rmdir(name, dir_fd=current_fd)
    finally:
        os.close(owned_fd)


if __name__ == "__main__":
    sanitize()
