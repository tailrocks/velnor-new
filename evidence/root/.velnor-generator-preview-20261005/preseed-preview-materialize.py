#!/usr/bin/python3
"""Materialize reviewed, checksum-pinned inputs into a private task rootfs.

This program performs file extraction and copying only. It does not execute
Mise, Cargo, Rust, validators, the Velnor helper, or installer scripts.
"""

import hashlib
import io
import json
import os
import posixpath
import ctypes
import stat
import sys
import tarfile
from pathlib import Path

TASK = "/root/.velnor-generator-preview-20261005"
PLAN = TASK + "/rootfs-stage-plan.json"
PROVENANCE = TASK + "/official-tool-asset-provenance.json"
ASSETS = TASK + "/assets"
STAGING = TASK + "/.rootfs-materializing-attempt-3"
ROOTFS = TASK + "/rootfs"
BASE_MANIFEST = TASK + "/rootfs-base-manifest.json"
PLAN_SHA256 = "ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f"
PROVENANCE_SHA256 = "67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137"

RUST_ARCHIVES = {
    "cargo-1.98.1-x86_64-unknown-linux-gnu.tar.xz": {
        "sha256": "ea1de9f9e23107d97ee2b41a72c552f34064a593da503789218387aee59f3ba4",
        "size": 11662108,
        "prefix": "cargo-1.98.1-x86_64-unknown-linux-gnu/cargo/",
        "metadata": {"LICENSE-APACHE", "LICENSE-MIT", "LICENSE-THIRD-PARTY", "README.md", "builder-config", "install.sh", "git-commit-hash", "git-commit-info", "rust-installer-version", "version", "components"},
    },
    "rustc-1.98.1-x86_64-unknown-linux-gnu.tar.xz": {
        "sha256": "e974f036b28565f37c0f3bd92ddefa809bee16c04f9dcf07b9ed96e05aaaf7c4",
        "size": 79727616,
        "prefix": "rustc-1.98.1-x86_64-unknown-linux-gnu/rustc/",
        "metadata": {"LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT", "README.md", "builder-config", "install.sh", "git-commit-hash", "git-commit-info", "rust-installer-version", "version", "components"},
    },
    "rust-std-1.98.1-x86_64-unknown-linux-gnu.tar.xz": {
        "sha256": "fa3ff450172a16c026944030230c5069947af93c728d9179971d44e5e0cfb561",
        "size": 30715684,
        "prefix": "rust-std-1.98.1-x86_64-unknown-linux-gnu/rust-std-x86_64-unknown-linux-gnu/",
        "metadata": {"LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT", "README.md", "builder-config", "install.sh", "git-commit-hash", "git-commit-info", "rust-installer-version", "version", "components"},
    },
}

VALIDATOR_ARCHIVES = {
    "actionlint_1.7.12_linux_amd64.tar.gz": {
        "archive_sha256": "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
        "archive_size": 2353908,
        "member_size": 6074530,
        "name": "actionlint",
        "version": "1.7.12",
        "member": "actionlint",
        "member_sha256": "c872d6db8c6bf83a8eaa704fc93999f027d55dffbc63b8a6abdccb47df5f4cd4",
        "destination": "/tools/validators/actionlint/1.7.12/bin/actionlint",
    },
    "shellcheck-v0.11.0.linux.x86_64.tar.xz": {
        "archive_sha256": "8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198",
        "archive_size": 2559196,
        "member_size": 16213136,
        "name": "shellcheck",
        "version": "0.11.0",
        "member": "shellcheck-v0.11.0/shellcheck",
        "member_sha256": "4da528ddb3a4d1b7b24a59d4e16eb2f5fd960f4bd9a3708a15baddbdf1d5a55b",
        "destination": "/tools/validators/shellcheck/0.11.0/bin/shellcheck",
    },
    "zizmor-x86_64-unknown-linux-gnu.tar.gz": {
        "archive_sha256": "e65324f4430c2717591937edcec90ccbefaf14c174f8ec9415e03ca875b46e1a",
        "archive_size": 9169512,
        "member_size": 26603008,
        "name": "zizmor",
        "version": "1.30.1",
        "member": "zizmor",
        "member_sha256": "d39db5594fd89d57fdadd744079e1b3a405f24ff312714904f8bd708c69f322f",
        "destination": "/tools/validators/zizmor/1.30.1/bin/zizmor",
    },
}


def fail(message):
    raise SystemExit("materializer: " + message)


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def open_root_regular(path, limit=1 << 30):
    if not path.startswith("/"):
        fail("absolute input path required")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        parts = path.split("/")[1:]
        for part in parts[:-1]:
            if part in ("", ".", ".."):
                fail("invalid input path")
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            st = os.fstat(child)
            if st.st_uid != 0 or st.st_gid != 0 or stat.S_IMODE(st.st_mode) & 0o022:
                fail("unsafe input directory")
            os.close(fd)
            fd = child
        name = parts[-1]
        src = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
        st = os.fstat(src)
        if (
            not stat.S_ISREG(st.st_mode)
            or st.st_nlink != 1
            or st.st_uid != 0
            or st.st_gid != 0
            or stat.S_IMODE(st.st_mode) & 0o022
            or st.st_size > limit
        ):
            os.close(src)
            fail("unsafe input file")
        return src
    finally:
        os.close(fd)


def read_root_regular(path, expected_sha, limit=1 << 30):
    src = open_root_regular(path, limit)
    try:
        digest = hashlib.sha256()
        blocks = []
        while True:
            block = os.read(src, 1 << 20)
            if not block:
                break
            digest.update(block)
            blocks.append(block)
        actual = digest.hexdigest()
        if actual != expected_sha:
            fail("input digest mismatch")
        return b"".join(blocks)
    finally:
        os.close(src)


def safe_rel_path(value):
    if not isinstance(value, str) or not value or value.startswith("/") or "\x00" in value:
        fail("unsafe relative path")
    parts = value.split("/")
    if any(part in ("", ".", "..") for part in parts):
        fail("unsafe relative path")
    return tuple(parts)


def destination_rel_path(value):
    if not isinstance(value, str) or not value.startswith("/") or value.startswith("//"):
        fail("rootfs destination must be one absolute path")
    return "/".join(safe_rel_path(value[1:]))


def validate_symlink_target(rel, target):
    rel_parts = safe_rel_path(rel)
    if not isinstance(target, str) or not target or "\x00" in target:
        fail("invalid rootfs symlink target")
    stack = [] if target.startswith("/") else list(rel_parts[:-1])
    for part in target.split("/"):
        if part in ("", "."):
            continue
        if part == "..":
            if not stack:
                fail("rootfs symlink escapes root")
            stack.pop()
        else:
            stack.append(part)


class TreeWriter:
    def __init__(self, root):
        self.root = root
        os.mkdir(root, 0o700)
        os.chown(root, 0, 0)
        os.chmod(root, 0o755)
        self.rootfd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        self.created = {""}

    def close(self):
        os.close(self.rootfd)

    def ensure_dir(self, rel, mode=0o755):
        parts = safe_rel_path(rel)
        fd = os.dup(self.rootfd)
        prefix = []
        try:
            for part in parts:
                prefix.append(part)
                joined = "/".join(prefix)
                try:
                    os.mkdir(part, mode if len(prefix) == len(parts) else 0o755, dir_fd=fd)
                except FileExistsError:
                    pass
                child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                st = os.fstat(child)
                if st.st_uid != 0 or st.st_gid != 0 or stat.S_IMODE(st.st_mode) & 0o022:
                    fail("unsafe staging directory")
                if joined not in self.created:
                    os.fchmod(child, mode if len(prefix) == len(parts) else 0o755)
                    self.created.add(joined)
                os.close(fd)
                fd = child
        finally:
            os.close(fd)

    def parentfd(self, rel):
        parts = safe_rel_path(rel)
        fd = os.dup(self.rootfd)
        try:
            for part in parts[:-1]:
                child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                os.close(fd)
                fd = child
            return fd, parts[-1]
        except BaseException:
            os.close(fd)
            raise

    def write_bytes(self, rel, data, mode):
        parent, name = self.parentfd(rel)
        try:
            fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode, dir_fd=parent)
            try:
                view = memoryview(data)
                while view:
                    written = os.write(fd, view)
                    view = view[written:]
                os.fchown(fd, 0, 0)
                os.fchmod(fd, mode)
            finally:
                os.close(fd)
        finally:
            os.close(parent)

    def copy_file(self, rel, source_fd, mode, expected_sha, expected_size=None):
        parent, name = self.parentfd(rel)
        out = -1
        digest = hashlib.sha256()
        size = 0
        try:
            out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode, dir_fd=parent)
            while True:
                block = os.read(source_fd, 1 << 20)
                if not block:
                    break
                digest.update(block)
                size += len(block)
                view = memoryview(block)
                while view:
                    written = os.write(out, view)
                    view = view[written:]
            if digest.hexdigest() != expected_sha or (expected_size is not None and size != expected_size):
                fail("copied input digest or size mismatch")
            os.fchown(out, 0, 0)
            os.fchmod(out, mode)
        finally:
            if out >= 0:
                os.close(out)
            os.close(parent)

    def symlink(self, rel, target):
        validate_symlink_target(rel, target)
        parent, name = self.parentfd(rel)
        try:
            os.symlink(target, name, dir_fd=parent)
        finally:
            os.close(parent)

    def null_device(self):
        parent, name = self.parentfd("dev/null")
        try:
            os.mknod(name, stat.S_IFCHR | 0o666, os.makedev(1, 3), dir_fd=parent)
            os.chown(name, 0, 0, dir_fd=parent, follow_symlinks=False)
            os.chmod(name, 0o666, dir_fd=parent, follow_symlinks=False)
        finally:
            os.close(parent)


def copy_host_entry(writer, record):
    rel = destination_rel_path(record["destination"])
    if "synthetic_content" in record:
        data = record["synthetic_content"].encode()
        if sha256_bytes(data) != record["sha256"] or len(data) != record["size"]:
            fail("synthetic content does not match plan")
    else:
        source = record.get("resolved_source") or record.get("source")
        fd = open_root_regular(source, limit=record["size"])
        try:
            st = os.fstat(fd)
            if (
                st.st_size != record["size"]
                or stat.S_IMODE(st.st_mode) & 0o022
            ):
                fail("unsafe source file")
            mode_value = record.get("mode") or record.get("source_mode") or "0644"
            mode = int(mode_value, 0) if mode_value.startswith("0o") else int(mode_value, 8)
            if mode & 0o7000 or mode & 0o022:
                fail("unsupported planned file mode")
            writer.copy_file(
                rel,
                fd,
                mode,
                record["sha256"],
                expected_size=record["size"],
            )
        finally:
            os.close(fd)
        return
    mode_value = record.get("mode") or record.get("source_mode") or "0644"
    mode = int(mode_value, 0) if mode_value.startswith("0o") else int(mode_value, 8)
    if mode & 0o7000 or mode & 0o022:
        fail("unsupported planned file mode")
    writer.write_bytes(rel, data, mode)


def classify_rust_member(member, description):
    member_parts = safe_rel_path(member.name)
    if member.issym() or member.islnk() or member.isdev() or member.isfifo():
        fail("Rust component archive contains a non-regular object")
    prefix = description["prefix"]
    archive_root = prefix.split("/", 1)[0]
    if member.name == archive_root and member.isdir():
        return ("ignored-root", "")
    if member.name == prefix[:-1] and member.isdir():
        return ("ignored-component-root", "")
    if member.name.startswith(prefix):
        rel = member.name[len(prefix):]
        if not rel:
            fail("empty Rust component member path")
        rel_parts = safe_rel_path(rel)
        if rel == "manifest.in" and member.isfile():
            return ("ignored-manifest", "")
        if member.isdir():
            return ("directory", "/".join(rel_parts))
        if member.isfile():
            return ("file", "/".join(rel_parts))
        fail("unsupported Rust archive entry")
    metadata_path = "/".join(member_parts[1:]) if len(member_parts) > 1 else ""
    if len(member_parts) == 2 and metadata_path in description["metadata"] and member.isfile():
        return ("ignored-metadata", "")
    fail("unexpected Rust archive path")


def validate_validator_members(members, description):
    if len(members) > 32:
        fail("validator archive member limit exceeded")
    selected = []
    for member in members:
        safe_rel_path(member.name)
        if not (member.isfile() or member.isdir()):
            fail("validator archive contains unsupported object")
        if member.name == description["member"] and member.isfile():
            selected.append(member)
    if len(selected) != 1:
        fail("validator archive member contract mismatch")
    return selected[0]


def validate_plan_paths(plan):
    directories = set()
    files = set()
    symlinks = set()
    leaves = set()
    for item in plan.get("rootfs_dirs", []):
        rel = destination_rel_path(item["path"])
        if rel in directories:
            fail("duplicate rootfs directory")
        directories.add(rel)
    for record in plan.get("rootfs_files", []):
        rel = destination_rel_path(record["destination"])
        if rel in files:
            fail("duplicate rootfs file destination")
        files.add(rel)
        if "synthetic_content" in record:
            data = record["synthetic_content"].encode()
            if hashlib.sha256(data).hexdigest() != record["sha256"] or len(data) != record["size"]:
                fail("synthetic content does not match plan")
        else:
            source = record.get("resolved_source") or record.get("source")
            data = read_root_regular(source, record["sha256"], limit=record["size"])
            if len(data) != record["size"]:
                fail("host file size differs from plan")
    for record in plan.get("rootfs_symlinks", []):
        rel = destination_rel_path(record["path"])
        validate_symlink_target(rel, record["target"])
        if rel in symlinks:
            fail("duplicate rootfs symlink")
        symlinks.add(rel)
    for record in plan.get("mise_links", []):
        rel = destination_rel_path(record["installation_path"])
        validate_symlink_target(rel, record["target"])
        if rel in symlinks:
            fail("duplicate Mise link destination")
        symlinks.add(rel)
    devices = plan.get("rootfs_devices", [])
    if len(devices) != 1:
        fail("rootfs device inventory mismatch")
    device = devices[0]
    device_rel = destination_rel_path(device["path"])
    if device_rel != "dev/null" or device.get("kind") != "character-device" or device.get("major") != 1 or device.get("minor") != 3 or device.get("mode") != "0666":
        fail("rootfs null-device contract mismatch")
    leaves.update(files)
    leaves.update(symlinks)
    leaves.add(device_rel)
    if len(leaves) != len(files) + len(symlinks) + 1:
        fail("rootfs destination path collision")
    for leaf in leaves:
        if leaf in directories or any(other.startswith(leaf + "/") for other in leaves | directories):
            fail("rootfs leaf conflicts with another destination")
    return {"directories": len(directories), "host_files": len(files), "symlinks": len(symlinks), "devices": len(devices)}


def preflight_archive_paths():
    rust_count = 0
    validator_count = 0
    rust_files = set()
    rust_directories = set()
    rust_expanded_size = 0
    for filename, description in RUST_ARCHIVES.items():
        data = read_root_regular(ASSETS + "/" + filename, description["sha256"], limit=128 << 20)
        try:
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:xz") as archive:
                members = archive.getmembers()
                if len(members) > 256:
                    fail("Rust component archive member limit exceeded")
                for member in members:
                    action, rel = classify_rust_member(member, description)
                    if action == "file":
                        if rel in rust_files or rel in rust_directories:
                            fail("Rust archive path collision")
                        rust_files.add(rel)
                        rust_expanded_size += member.size
                    elif action == "directory":
                        if rel in rust_files:
                            fail("Rust archive path collision")
                        rust_directories.add(rel)
                    rust_count += 1
        finally:
            del data
    if rust_expanded_size > 700 << 20:
        fail("Rust toolchain expanded-size bound exceeded")
    if not {"bin/cargo", "bin/rustc", "bin/rustdoc", "lib/libLLVM.so.22.1-rust-1.98.1-stable"}.issubset(rust_files):
        fail("Rust toolchain lacks required runtime files")
    if not any(path.startswith("lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd-") and path.endswith(".rlib") for path in rust_files):
        fail("Rust standard library target is missing")
    for filename, description in VALIDATOR_ARCHIVES.items():
        data = read_root_regular(ASSETS + "/" + filename, description["archive_sha256"], limit=16 << 20)
        try:
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:*") as archive:
                members = archive.getmembers()
                member = validate_validator_members(members, description)
                if member.size != description["member_size"]:
                    fail("validator executable size mismatch")
                stream = archive.extractfile(member)
                if stream is None:
                    fail("validator archive member unreadable")
                digest = hashlib.sha256()
                size = 0
                try:
                    while True:
                        block = stream.read(1 << 20)
                        if not block:
                            break
                        digest.update(block)
                        size += len(block)
                finally:
                    stream.close()
                if size != description["member_size"] or digest.hexdigest() != description["member_sha256"]:
                    fail("validator member bytes differ from provenance")
                validator_count += len(members)
        finally:
            del data
    return {"rust_archive_members": rust_count, "validator_archive_members": validator_count}


def preflight_staging_inputs(plan):
    plan_counts = validate_plan_paths(plan)
    archive_counts = preflight_archive_paths()
    return {**plan_counts, **archive_counts}


def verify_asset_provenance(plan, provenance):
    if plan.get("schema") != 1 or provenance.get("schema") != 1:
        fail("asset contract schema mismatch")
    if plan.get("tool_provenance_sha256") != PROVENANCE_SHA256:
        fail("asset contract plan reference mismatch")

    mise = provenance.get("mise")
    expected_mise_sha = "2fd6b7eb9889f13e07dc3fdb6271c05815b81e03da86ff9f875de0b5c473afb8"
    expected_mise_path = "/root/.local/share/mise/installs/aqua-jdx-mise/2026.9.18/mise/bin/mise"
    if (
        not isinstance(mise, dict)
        or mise.get("local_path") != expected_mise_path
        or mise.get("official_sha256") != expected_mise_sha
        or mise.get("local_sha256") != expected_mise_sha
        or mise.get("local_size") != mise.get("size")
        or mise.get("tag") != "v2026.9.18"
    ):
        fail("Mise provenance contract mismatch")
    mise_fd = open_root_regular(expected_mise_path, limit=256 << 20)
    try:
        if os.fstat(mise_fd).st_size != mise["local_size"]:
            fail("Mise provenance size mismatch")
    finally:
        os.close(mise_fd)

    rust = provenance.get("rust")
    if not isinstance(rust, dict) or rust.get("version") != "1.98.1" or rust.get("target") != "x86_64-unknown-linux-gnu":
        fail("Rust provenance contract mismatch")
    components = rust.get("components")
    if not isinstance(components, list) or len(components) != len(RUST_ARCHIVES):
        fail("Rust component inventory mismatch")
    components_by_archive = {}
    for component in components:
        if not isinstance(component, dict) or component.get("archive") in components_by_archive:
            fail("duplicate Rust component provenance")
        components_by_archive[component.get("archive")] = component
    if set(components_by_archive) != set(RUST_ARCHIVES):
        fail("Rust archive names do not match the extraction plan")
    for filename, description in RUST_ARCHIVES.items():
        component = components_by_archive[filename]
        if (
            component.get("sha256_local") != description["sha256"]
            or component.get("sha256_official_channel") != description["sha256"]
            or component.get("size") != description["size"]
        ):
            fail("Rust component digest or size provenance mismatch")
        archive_fd = open_root_regular(ASSETS + "/" + filename, limit=128 << 20)
        try:
            if os.fstat(archive_fd).st_size != component["size"]:
                fail("Rust component file size mismatch")
        finally:
            os.close(archive_fd)
    channel_manifest = rust.get("manifest_path")
    if channel_manifest != ASSETS + "/channel-rust-1.98.1.toml":
        fail("Rust channel manifest path mismatch")
    read_root_regular(channel_manifest, rust.get("manifest_sha256"), limit=2 << 20)

    validators = provenance.get("validators")
    if not isinstance(validators, list) or len(validators) != len(VALIDATOR_ARCHIVES):
        fail("validator inventory mismatch")
    validators_by_asset = {}
    for validator in validators:
        if not isinstance(validator, dict) or validator.get("asset") in validators_by_asset:
            fail("duplicate validator provenance")
        validators_by_asset[validator.get("asset")] = validator
    if set(validators_by_asset) != set(VALIDATOR_ARCHIVES):
        fail("validator archive names do not match extraction plan")
    for filename, description in VALIDATOR_ARCHIVES.items():
        validator = validators_by_asset[filename]
        if (
            validator.get("name") != description["name"]
            or validator.get("version") != description["version"]
            or validator.get("official_sha256") != description["archive_sha256"]
            or validator.get("local_archive_sha256") != description["archive_sha256"]
            or validator.get("extracted_binary_sha256") != description["member_sha256"]
        ):
            fail("validator digest or identity provenance mismatch")
        archive_fd = open_root_regular(ASSETS + "/" + filename, limit=16 << 20)
        try:
            if os.fstat(archive_fd).st_size != description["archive_size"]:
                fail("validator archive file size mismatch")
        finally:
            os.close(archive_fd)


def copy_rust_components(writer):
    wrote = {}
    total_bytes = 0
    for filename, description in RUST_ARCHIVES.items():
        archive_path = ASSETS + "/" + filename
        expected_sha = description["sha256"]
        archive_bytes = read_root_regular(archive_path, expected_sha, limit=128 << 20)
        try:
            with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:xz") as archive:
                members = archive.getmembers()
                if len(members) > 256:
                    fail("Rust component archive member limit exceeded")
                for member in members:
                    action, rel = classify_rust_member(member, description)
                    if action.startswith("ignored-"):
                        continue
                    out_rel = "tools/rust-1.98.1/" + rel
                    if action == "directory":
                        writer.ensure_dir(out_rel, 0o755)
                        continue
                    if action != "file":
                        fail("unknown Rust archive disposition")
                    mode = 0o755 if member.mode & 0o111 else 0o644
                    source = archive.extractfile(member)
                    if source is None:
                        fail("Rust archive file could not be read")
                    digest = hashlib.sha256()
                    writer.ensure_dir(posixpath.dirname(out_rel))
                    parent, name = writer.parentfd(out_rel)
                    outfd = -1
                    try:
                        outfd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode, dir_fd=parent)
                        while True:
                            block = source.read(1 << 20)
                            if not block:
                                break
                            digest.update(block)
                            total_bytes += len(block)
                            view = memoryview(block)
                            while view:
                                written = os.write(outfd, view)
                                view = view[written:]
                        if rel in wrote:
                            fail("Rust archive component path collision")
                        wrote[rel] = digest.hexdigest()
                        os.fchown(outfd, 0, 0)
                        os.fchmod(outfd, mode)
                    finally:
                        if outfd >= 0:
                            os.close(outfd)
                        os.close(parent)
                        source.close()
        finally:
            del archive_bytes
    if not {"bin/cargo", "bin/rustc", "bin/rustdoc", "lib/libLLVM.so.22.1-rust-1.98.1-stable"}.issubset(wrote):
        fail("Rust toolchain lacks required runtime files")
    if not any(path.startswith("lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd-") and path.endswith(".rlib") for path in wrote):
        fail("Rust standard library target is missing")
    if total_bytes > 700 << 20:
        fail("Rust toolchain expanded-size bound exceeded")


def extract_validator(writer, filename, description):
    archive_path = ASSETS + "/" + filename
    data = read_root_regular(archive_path, description["archive_sha256"], limit=16 << 20)
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:*") as archive:
            members = archive.getmembers()
            member = validate_validator_members(members, description)
            stream = archive.extractfile(member)
            if stream is None:
                fail("validator archive member unreadable")
            digest = hashlib.sha256()
            parent, name = writer.parentfd(destination_rel_path(description["destination"]))
            outfd = -1
            try:
                outfd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o755, dir_fd=parent)
                size = 0
                while True:
                    block = stream.read(1 << 20)
                    if not block:
                        break
                    digest.update(block)
                    size += len(block)
                    view = memoryview(block)
                    while view:
                        written = os.write(outfd, view)
                        view = view[written:]
                if digest.hexdigest() != description["member_sha256"] or size != description["member_size"]:
                    fail("validator executable digest mismatch")
                os.fchown(outfd, 0, 0)
                os.fchmod(outfd, 0o755)
            finally:
                if outfd >= 0:
                    os.close(outfd)
                os.close(parent)
                stream.close()
    finally:
        del data


def tree_hash(root):
    records = []

    def walk(fd, prefix):
        for name in sorted(os.listdir(fd), key=os.fsencode):
            if name in (".", "..") or "/" in name:
                fail("invalid rootfs directory entry")
            path = (prefix + "/" + name) if prefix else "/" + name
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            mode = stat.S_IMODE(st.st_mode)
            if st.st_uid != 0 or st.st_gid != 0:
                fail("rootfs owner mismatch")
            if stat.S_ISCHR(st.st_mode) and path == "/dev/null" and os.major(st.st_rdev) == 1 and os.minor(st.st_rdev) == 3:
                if mode != 0o666:
                    fail("unexpected null device mode")
                records.append({"path": path, "type": "character-device", "mode": mode, "uid": 0, "gid": 0, "major": 1, "minor": 3})
                continue
            if stat.S_ISLNK(st.st_mode):
                target = os.readlink(name, dir_fd=fd)
                normalized = posixpath.normpath(target if target.startswith("/") else posixpath.join(posixpath.dirname(path), target))
                if not normalized.startswith("/") or normalized == "/.." or normalized.startswith("/../"):
                    fail("rootfs symlink escape")
                records.append({"path": path, "type": "symlink", "mode": mode, "uid": 0, "gid": 0, "target": target})
                continue
            if mode & 0o022:
                fail("rootfs writable-by-group-or-world path")
            if stat.S_ISDIR(st.st_mode):
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                try:
                    opened = os.fstat(child)
                    if not stat.S_ISDIR(opened.st_mode) or (opened.st_dev, opened.st_ino) != (st.st_dev, st.st_ino):
                        fail("rootfs directory changed during hashing")
                    records.append({"path": path, "type": "directory", "mode": mode, "uid": 0, "gid": 0})
                    walk(child, path)
                finally:
                    os.close(child)
            elif stat.S_ISREG(st.st_mode):
                if st.st_nlink != 1:
                    fail("rootfs hard link rejected")
                child = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                try:
                    opened = os.fstat(child)
                    if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (st.st_dev, st.st_ino):
                        fail("rootfs file changed during hashing")
                    digest = hashlib.sha256()
                    while True:
                        block = os.read(child, 1 << 20)
                        if not block:
                            break
                        digest.update(block)
                    records.append({"path": path, "type": "file", "mode": mode, "uid": 0, "gid": 0, "size": st.st_size, "sha256": digest.hexdigest()})
                finally:
                    os.close(child)
            else:
                fail("unsupported rootfs entry")

    rootfd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    rootst = os.fstat(rootfd)
    if not stat.S_ISDIR(rootst.st_mode) or rootst.st_uid != 0 or rootst.st_gid != 0 or stat.S_IMODE(rootst.st_mode) != 0o755:
        os.close(rootfd)
        fail("rootfs root mode mismatch")
    records.append({"path": "/", "type": "directory", "mode": 0o755, "uid": 0, "gid": 0})
    try:
        walk(rootfd, "")
    finally:
        os.close(rootfd)
    encoded = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
    return hashlib.sha256(encoded).hexdigest()


def main():
    if sys.flags.isolated != 1 or sys.flags.no_site != 1:
        fail("start with python3 -I -S")
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run only under env -i PATH=/usr/bin:/bin LC_ALL=C")
    libc = ctypes.CDLL(None, use_errno=True)
    close_range = getattr(libc, "close_range", None)
    if close_range is None:
        fail("libc close_range is required")
    close_range.argtypes = (ctypes.c_uint, ctypes.c_uint, ctypes.c_uint)
    close_range.restype = ctypes.c_int
    if close_range(3, 0xFFFFFFFF, 0) != 0:
        fail("close_range failed")
    nullfd = os.open("/dev/null", os.O_RDWR | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        nullst = os.fstat(nullfd)
        if not stat.S_ISCHR(nullst.st_mode) or os.major(nullst.st_rdev) != 1 or os.minor(nullst.st_rdev) != 3:
            fail("host null device identity mismatch")
        for target in (0, 1, 2):
            os.dup2(nullfd, target)
    finally:
        os.close(nullfd)
    if os.geteuid() != 0 or os.getegid() != 0:
        fail("effective uid must be root for private rootfs ownership")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("host identity mismatch")
    taskst = os.stat(TASK, follow_symlinks=False)
    if not stat.S_ISDIR(taskst.st_mode) or taskst.st_uid != 0 or taskst.st_gid != 0 or stat.S_IMODE(taskst.st_mode) != 0o700:
        fail("private task directory identity mismatch")
    logfd = os.open(TASK + "/materialize.log", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        logst = os.fstat(logfd)
        if not stat.S_ISREG(logst.st_mode) or logst.st_uid != 0 or logst.st_gid != 0 or logst.st_nlink != 1:
            fail("unsafe materialization log")
        os.fchown(logfd, 0, 0)
        os.fchmod(logfd, 0o600)
        os.dup2(logfd, 1)
        os.dup2(logfd, 2)
    finally:
        os.close(logfd)
    if os.path.lexists(ROOTFS) or os.path.lexists(STAGING) or os.path.lexists(BASE_MANIFEST):
        fail("refuse to replace existing staged state")

    plan_bytes = read_root_regular(PLAN, PLAN_SHA256, limit=1 << 20)
    provenance_bytes = read_root_regular(PROVENANCE, PROVENANCE_SHA256, limit=1 << 20)
    plan = json.loads(plan_bytes)
    provenance = json.loads(provenance_bytes)
    if plan.get("schema") != 1 or plan.get("tool_provenance_sha256") != PROVENANCE_SHA256:
        fail("plan provenance mismatch")
    verify_asset_provenance(plan, provenance)
    path_counts = preflight_staging_inputs(plan)

    writer = TreeWriter(STAGING)
    try:
        for directory in sorted(plan.get("rootfs_dirs", []), key=lambda item: item["path"].count("/")):
            writer.ensure_dir(destination_rel_path(directory["path"]), int(directory["mode"], 8))
        for record in plan.get("rootfs_files", []):
            writer.ensure_dir(posixpath.dirname(destination_rel_path(record["destination"])))
            copy_host_entry(writer, record)
        writer.null_device()
        for record in plan.get("rootfs_symlinks", []):
            writer.symlink(destination_rel_path(record["path"]), record["target"])
        for link in plan.get("mise_links", []):
            writer.symlink(destination_rel_path(link["installation_path"]), link["target"])

        mise = provenance["mise"]
        mise_bytes = read_root_regular(mise["local_path"], mise["local_sha256"], limit=256 << 20)
        writer.ensure_dir("tools/mise/bin")
        writer.write_bytes("tools/mise/bin/mise", mise_bytes, 0o755)

        for filename, description in VALIDATOR_ARCHIVES.items():
            extract_validator(writer, filename, description)
        copy_rust_components(writer)

        for placeholder in ("tools/velnor-actions", "tools/run-preview.sh"):
            if os.stat(os.path.join(STAGING, placeholder), follow_symlinks=False).st_size != 0:
                fail("unexpected runtime placeholder content")
        base_digest = tree_hash(STAGING)
    finally:
        writer.close()

    os.chmod(STAGING, 0o755)
    os.rename(STAGING, ROOTFS)
    digest = tree_hash(ROOTFS)
    if digest != base_digest:
        fail("rootfs digest changed during final rename")
    manifest = {
        "schema": 1,
        "status": "BASE_MATERIALIZED; cargo seed and final stage manifest are still absent",
        "plan_sha256": PLAN_SHA256,
        "tool_provenance_sha256": PROVENANCE_SHA256,
        "materializer_sha256": sha256_bytes(Path(__file__).read_bytes()),
        "base_rootfs_tree_sha256": digest,
    }
    data = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode()
    fd = os.open(BASE_MANIFEST, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        view = memoryview(data)
        while view:
            written = os.write(fd, view)
            view = view[written:]
        os.fchown(fd, 0, 0)
        os.fchmod(fd, 0o600)
    finally:
        os.close(fd)
    print("base rootfs staged; no tools were executed")
    print("path_preflight=" + json.dumps(path_counts, sort_keys=True))
    print("base_manifest_sha256=" + sha256_bytes(data))
    print("base_rootfs_tree_sha256=" + digest)


if __name__ == "__main__":
    main()
