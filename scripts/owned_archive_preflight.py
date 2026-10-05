"""Bind Python tar parsing to a fingerprinted native preflight in this checkout."""

import errno
import hashlib
import os
from pathlib import Path
import stat
import subprocess

INPUT_MANIFEST = "scripts/archive-guard-inputs.txt"
INPUT_MANIFEST_LIMIT = 32 * 1024
SOURCE_FILE_LIMIT = 16 * 1024 * 1024
SOURCE_TREE_LIMIT = 32 * 1024 * 1024
SOURCE_COUNT_LIMIT = 512
SOURCE_DIRECTORY_LIMIT = 512
SOURCE_ENTRY_LIMIT = 512
BUILD_COMMAND = "bash scripts/build-owned-archive-guard.sh"
DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK


def _guard_path():
    source_file = Path(os.path.abspath(__file__))
    repository = source_file.parent.parent
    if source_file.relative_to(repository).as_posix() != "scripts/owned_archive_preflight.py":
        raise ValueError("archive preflight source has untrusted provenance")
    root_fd = _open_checkout(repository)
    try:
        _check_git_marker(root_fd)
        _trusted_checkout_file(root_fd, "scripts/owned_archive_preflight.py",
                               "archive preflight source")
        fingerprint = _source_fingerprint_at(root_fd)
        state_fd = _open_child_directory(root_fd, ".velnor", "archive guard state")
        try:
            guard_state_fd = _open_child_directory(
                state_fd, "archive-guard", "archive guard state")
        finally:
            os.close(state_fd)
        try:
            binary_fd = _open_child_directory(
                guard_state_fd, "bin", "native archive guard binary")
        finally:
            os.close(guard_state_fd)
        try:
            guard = (repository / ".velnor" / "archive-guard" / "bin"
                     / "velnor-archive-guard")
            if _matches_fingerprint(binary_fd, "velnor-archive-guard", guard, fingerprint):
                return guard
        finally:
            os.close(binary_fd)
        raise ValueError("native archive guard is stale or not built; run: " + BUILD_COMMAND)
    finally:
        os.close(root_fd)


def _open_checkout(repository):
    if not hasattr(os, "O_DIRECTORY") or not hasattr(os, "O_NOFOLLOW"):
        raise ValueError("archive guard requires no-follow directory support")
    repository = Path(os.path.abspath(repository))
    try:
        if repository.resolve(strict=True) != repository:
            raise ValueError("archive guard checkout path contains a symlink")
        current = os.open(repository.anchor, DIRECTORY_FLAGS)
    except OSError as error:
        raise ValueError("archive guard requires a trusted repository checkout") from error
    try:
        components = repository.parts[1:]
        for index, component in enumerate(components):
            following = None
            try:
                following = os.open(component, DIRECTORY_FLAGS, dir_fd=current)
                metadata = os.fstat(following)
                if index + 1 == len(components):
                    _require_owned_directory(metadata, "repository checkout")
                else:
                    _require_safe_ancestor(metadata)
            except (OSError, ValueError):
                if following is not None:
                    os.close(following)
                raise
            os.close(current)
            current = following
        return current
    except (OSError, ValueError) as error:
        os.close(current)
        if isinstance(error, ValueError):
            raise
        raise ValueError("archive guard requires a trusted repository checkout") from error


def _require_safe_ancestor(metadata):
    owner_is_trusted = metadata.st_uid in (0, os.getuid())
    shared_writable = metadata.st_mode & 0o022
    root_sticky = metadata.st_uid == 0 and metadata.st_mode & stat.S_ISVTX
    if (not stat.S_ISDIR(metadata.st_mode) or not owner_is_trusted
            or (shared_writable and not root_sticky)):
        raise ValueError("archive guard checkout ancestry has untrusted provenance")


def _require_owned_directory(metadata, label):
    if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
            or metadata.st_mode & 0o022):
        raise ValueError(label + " directory has untrusted provenance")


def _open_child_directory(parent_fd, name, label, optional=False):
    try:
        descriptor = os.open(name, DIRECTORY_FLAGS, dir_fd=parent_fd)
    except FileNotFoundError:
        if optional:
            return None
        raise ValueError("native archive guard is not built; run: " + BUILD_COMMAND)
    except OSError as error:
        raise ValueError(label + " directory is unavailable") from error
    try:
        _require_owned_directory(os.fstat(descriptor), label)
        return descriptor
    except OSError as error:
        os.close(descriptor)
        raise ValueError(label + " directory is unavailable") from error
    except ValueError:
        os.close(descriptor)
        raise


def _check_git_marker(root_fd):
    try:
        metadata = os.stat(".git", dir_fd=root_fd, follow_symlinks=False)
    except OSError as error:
        raise ValueError("archive guard requires a trusted repository checkout") from error
    trusted_kind = stat.S_ISDIR(metadata.st_mode) or stat.S_ISREG(metadata.st_mode)
    if (not trusted_kind or metadata.st_uid != os.getuid() or metadata.st_mode & 0o022
            or stat.S_ISLNK(metadata.st_mode)):
        raise ValueError("archive guard requires a trusted repository checkout")


def _open_relative_parent(root_fd, relative, label, optional=False):
    components = Path(relative).parts
    if Path(relative).is_absolute() or not components or any(
            component in ("", ".", "..") for component in components):
        raise ValueError(label + " path is outside the repository")
    parent = os.dup(root_fd)
    try:
        for component in components[:-1]:
            following = os.open(component, DIRECTORY_FLAGS, dir_fd=parent)
            try:
                _require_owned_directory(os.fstat(following), label)
            except (OSError, ValueError):
                os.close(following)
                raise
            os.close(parent)
            parent = following
        return parent, components[-1]
    except (OSError, ValueError) as error:
        os.close(parent)
        if optional and isinstance(error, OSError) and error.errno == errno.ENOENT:
            return None, None
        if isinstance(error, ValueError):
            raise
        raise ValueError(label + " parent directory has untrusted provenance") from error


def _trusted_checkout_file(root_fd, relative, label):
    parent, name = _open_relative_parent(root_fd, relative, label)
    try:
        descriptor = os.open(name, FILE_FLAGS, dir_fd=parent)
        try:
            before = os.fstat(descriptor)
            if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
                    or before.st_uid != os.getuid() or before.st_mode & 0o022
                    or before.st_size > SOURCE_FILE_LIMIT):
                raise ValueError(label + " has untrusted provenance")
            with os.fdopen(descriptor, "rb", closefd=False) as stream:
                data = stream.read(SOURCE_FILE_LIMIT + 1)
            after = os.fstat(descriptor)
            named = os.stat(name, dir_fd=parent, follow_symlinks=False)
            if _file_identity(before) != _file_identity(after) or (
                    before.st_dev, before.st_ino) != (named.st_dev, named.st_ino):
                raise ValueError(label + " changed during verification")
            if len(data) > SOURCE_FILE_LIMIT:
                raise ValueError(label + " exceeds size limit")
            return data
        finally:
            os.close(descriptor)
    except OSError as error:
        raise ValueError(label + " is missing or has untrusted provenance") from error
    finally:
        os.close(parent)


def _file_identity(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns,
            metadata.st_ctime_ns, metadata.st_mode, metadata.st_nlink, metadata.st_uid)


def _source_fingerprint(repository):
    root_fd = _open_checkout(repository)
    try:
        return _source_fingerprint_at(root_fd)
    finally:
        os.close(root_fd)


def _source_fingerprint_at(root_fd):
    digest = hashlib.sha256()
    manifest = _trusted_checkout_file(root_fd, INPUT_MANIFEST, "archive input manifest")
    if len(manifest) > INPUT_MANIFEST_LIMIT:
        raise ValueError("archive input manifest exceeds size limit")
    digest.update(INPUT_MANIFEST.encode())
    digest.update(b"\0")
    digest.update(manifest)
    entries = _parse_input_manifest(manifest)
    total = len(manifest)
    seen = set()
    for kind, relative in entries:
        inputs = _tree_files(root_fd, relative) if kind == "tree" else [relative]
        for source in inputs:
            if source in seen or len(seen) >= SOURCE_COUNT_LIMIT:
                raise ValueError("archive input manifest resolves duplicate or too many files")
            seen.add(source)
            optional = kind == "optional"
            data = (_optional_checkout_file(root_fd, source, "archive guard build input")
                    if optional else _trusted_checkout_file(
                        root_fd, source, "archive guard source"))
            if data is None:
                digest.update(source.encode())
                digest.update(b"\0\0")
                continue
            total += len(data)
            if total > SOURCE_TREE_LIMIT:
                raise ValueError("archive guard source tree exceeds size limit")
            digest.update(source.encode())
            digest.update(b"\0\1" if optional else b"\0")
            digest.update(data)
    return digest.hexdigest()


def _parse_input_manifest(manifest):
    try:
        text = manifest.decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError("archive input manifest is not ASCII") from error
    if not text.endswith("\n"):
        raise ValueError("archive input manifest must end with a newline")
    entries = []
    seen = set()
    for line in text.splitlines():
        parts = line.split(" ")
        if len(parts) != 2 or parts[0] not in ("file", "optional", "tree"):
            raise ValueError("archive input manifest entry is malformed")
        kind, relative = parts
        path = Path(relative)
        if (not relative or "\\" in relative or path.is_absolute()
                or any(part in ("", ".", "..") for part in path.parts)
                or relative in seen):
            raise ValueError("archive input manifest path is unsafe or duplicated")
        seen.add(relative)
        entries.append((kind, relative))
    if not entries or len(entries) > SOURCE_COUNT_LIMIT:
        raise ValueError("archive input manifest entry count is invalid")
    return entries


def _tree_files(root_fd, relative):
    pending = [relative]
    files = []
    directories = 0
    discovered = 0
    while pending:
        directories += 1
        if directories > SOURCE_DIRECTORY_LIMIT:
            raise ValueError("archive source tree directory count exceeded")
        directory = pending.pop()
        parent, name = _open_relative_parent(root_fd, directory, "archive source tree")
        try:
            descriptor = os.open(name, DIRECTORY_FLAGS, dir_fd=parent)
            try:
                _require_owned_directory(os.fstat(descriptor), "archive source tree")
                with os.scandir(descriptor) as entries:
                    for entry in entries:
                        discovered += 1
                        if discovered > SOURCE_ENTRY_LIMIT:
                            raise ValueError("archive source tree entry count exceeded")
                        name = entry.name
                        metadata = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
                        path = directory + "/" + name
                        if stat.S_ISLNK(metadata.st_mode):
                            raise ValueError("archive source tree contains a symlink")
                        if stat.S_ISDIR(metadata.st_mode):
                            _require_owned_directory(metadata, "archive source tree")
                            if len(pending) >= SOURCE_DIRECTORY_LIMIT:
                                raise ValueError("archive source tree pending directory count exceeded")
                            pending.append(path)
                        elif stat.S_ISREG(metadata.st_mode):
                            path.encode("utf-8")
                            if len(files) >= SOURCE_COUNT_LIMIT:
                                raise ValueError("archive source tree file count exceeded")
                            files.append(path)
                        else:
                            raise ValueError("archive source tree contains a special file")
            finally:
                os.close(descriptor)
        except OSError as error:
            raise ValueError("archive source tree is unavailable") from error
        finally:
            os.close(parent)
    return sorted(files)


def _optional_checkout_file(root_fd, relative, label):
    parent, name = _open_relative_parent(root_fd, relative, label, optional=True)
    if parent is None:
        return None
    try:
        return _read_checkout_file_at(parent, name, label)
    finally:
        os.close(parent)


def _read_checkout_file_at(parent, name, label):
    try:
        descriptor = os.open(name, FILE_FLAGS, dir_fd=parent)
    except FileNotFoundError:
        return None
    try:
        metadata = os.fstat(descriptor)
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1
                or metadata.st_uid != os.getuid() or metadata.st_mode & 0o022
                or metadata.st_size > SOURCE_FILE_LIMIT):
            raise ValueError(label + " has untrusted provenance")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            data = stream.read(SOURCE_FILE_LIMIT + 1)
        if len(data) > SOURCE_FILE_LIMIT:
            raise ValueError(label + " exceeds size limit")
        after = os.fstat(descriptor)
        named = os.stat(name, dir_fd=parent, follow_symlinks=False)
        if _file_identity(metadata) != _file_identity(after) or (
                metadata.st_dev, metadata.st_ino) != (named.st_dev, named.st_ino):
            raise ValueError(label + " changed during verification")
        return data
    finally:
        os.close(descriptor)


def _matches_fingerprint(parent_fd, name, guard_path, expected):
    try:
        descriptor = os.open(name, FILE_FLAGS, dir_fd=parent_fd)
    except FileNotFoundError:
        return False
    except OSError as error:
        if error.errno == errno.ELOOP:
            raise ValueError("native archive guard executable has untrusted provenance") from error
        raise ValueError("native archive guard executable is unavailable") from error
    try:
        metadata = os.fstat(descriptor)
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1
                or metadata.st_uid != os.getuid() or metadata.st_mode & 0o022
                or not metadata.st_mode & 0o111):
            raise ValueError("native archive guard executable has untrusted provenance")
    finally:
        os.close(descriptor)
    try:
        result = subprocess.run([str(guard_path), "--fingerprint"], stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, check=False, timeout=10,
                                stdin=subprocess.DEVNULL, env={}, close_fds=True)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError("native archive guard identity check failed") from error
    return (result.returncode == 0 and result.stdout == (expected + "\n").encode()
            and not result.stderr)


def preflight_archive(data, mode):
    """Run the fixed in-checkout Rust guard on the exact bytes Python will parse."""
    if not isinstance(data, bytes):
        raise ValueError("archive preflight requires an immutable byte snapshot")
    command = [str(_guard_path()), mode]
    try:
        result = subprocess.run(command, input=data, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, check=False, timeout=300,
                                env={}, close_fds=True)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError("native archive preflight could not complete") from error
    if result.returncode != 0 or result.stdout:
        diagnostic = result.stderr.decode("utf-8", errors="replace").strip()
        raise ValueError("native archive preflight rejected input" +
                         (": " + diagnostic if diagnostic else ""))
