#!/usr/bin/python3
"""Create the private Mise/Rustup homes for the pinned Rust 1.98.1 toolchain."""
import hashlib
import json
import os
import pathlib
import stat
import sys
import tempfile

UID = 65534
GID = 65534
TOOLCHAIN = "1.98.1-x86_64-unknown-linux-gnu"
ALIAS = "1.98.1"
TOOLCHAIN_PATH = "/tools/rust-1.98.1"
PROXY_DIR = "/tools/rustup-proxy"
RUSTUP_SHA256 = "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c"
PROXY_NAMES = (
    "cargo", "cargo-clippy", "cargo-fmt", "clippy-driver",
    "rustc", "rustdoc", "rustfmt", "rustup",
)
SETTINGS = (
    'version = "12"\n'
    'default_toolchain = "1.98.1-x86_64-unknown-linux-gnu"\n'
    'profile = "default"\n'
).encode("ascii")
FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC


class GuardError(Exception):
    pass


def fail(message):
    raise GuardError(message)


def identity(fd, uid, gid, mode, label):
    item = os.fstat(fd)
    if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode)) != (uid, gid, mode):
        fail("unsafe directory: " + label)


def open_abs_dir(path):
    parts = pathlib.PurePosixPath(path)
    if not parts.is_absolute() or any(item in ("", ".", "..") for item in parts.parts[1:]):
        fail("unsafe absolute directory")
    fd = os.open("/", FLAGS)
    try:
        for name in parts.parts[1:]:
            child = os.open(name, FLAGS, dir_fd=fd)
            item = os.fstat(child)
            if stat.S_IMODE(item.st_mode) & 0o022:
                os.close(child)
                fail("writable path component: " + name)
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def ensure_dir(parent_fd, name, uid, gid, mode, label):
    created = False
    try:
        os.mkdir(name, mode, dir_fd=parent_fd)
        created = True
    except FileExistsError:
        pass
    fd = os.open(name, FLAGS, dir_fd=parent_fd)
    try:
        item = os.fstat(fd)
        if created and os.geteuid() == 0:
            os.fchown(fd, uid, gid)
            item = os.fstat(fd)
        if (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode)) != (uid, gid, mode):
            fail("unsafe directory: " + label)
        return fd
    except BaseException:
        os.close(fd)
        raise


def ensure_link(parent_fd, name, target, uid, gid, label):
    try:
        os.symlink(target, name, dir_fd=parent_fd)
    except FileExistsError:
        pass
    item = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if not stat.S_ISLNK(item.st_mode):
        fail("unexpected non-link: " + label)
    if (item.st_uid, item.st_gid) != (uid, gid):
        if os.geteuid() != 0:
            fail("unexpected link owner: " + label)
        os.chown(name, uid, gid, dir_fd=parent_fd, follow_symlinks=False)
    if os.readlink(name, dir_fd=parent_fd) != target:
        fail("unexpected link target: " + label)


def verify_file(parent_fd, name, uid, gid, mode, label, digest=None):
    fd = os.open(name, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (uid, gid, mode, 1):
            fail("unsafe regular file: " + label)
        hasher = hashlib.sha256()
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            hasher.update(block)
        actual = hasher.hexdigest()
        if digest is not None and actual != digest:
            fail("file digest mismatch: " + label)
        return actual
    finally:
        os.close(fd)


def verify_toolchain(tools_fd, rustup_sha256):
    proxy_fd = os.open("rustup-proxy", FLAGS, dir_fd=tools_fd)
    rust_fd = os.open("rust-1.98.1", FLAGS, dir_fd=tools_fd)
    mise_data_fd = os.open("mise-data", FLAGS, dir_fd=tools_fd)
    try:
        identity(tools_fd, 0, 0, 0o755, "/tools")
        identity(proxy_fd, 0, 0, 0o755, "Rustup proxy directory")
        if set(os.listdir(proxy_fd)) != set(PROXY_NAMES):
            fail("Rustup proxy inventory mismatch")
        for name in set(PROXY_NAMES) - {"rustup"}:
            item = os.stat(name, dir_fd=proxy_fd, follow_symlinks=False)
            if not stat.S_ISLNK(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0) or os.readlink(name, dir_fd=proxy_fd) != "rustup":
                fail("invalid Rustup proxy alias: " + name)
        verify_file(proxy_fd, "rustup", 0, 0, 0o555, "Rustup executable", rustup_sha256)
        identity(rust_fd, 0, 0, 0o755, "Rust toolchain")
        toolchain_bin = os.open("bin", FLAGS, dir_fd=rust_fd)
        try:
            for name in ("cargo", "rustc"):
                item = os.stat(name, dir_fd=toolchain_bin, follow_symlinks=False)
                if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (0, 0, 0o755, 1):
                    fail("invalid Rust toolchain executable: " + name)
        finally:
            os.close(toolchain_bin)
        mise_installs = os.open("installs", FLAGS, dir_fd=mise_data_fd)
        try:
            mise_rust = os.open("rust", FLAGS, dir_fd=mise_installs)
            try:
                item = os.stat(ALIAS, dir_fd=mise_rust, follow_symlinks=False)
                if not stat.S_ISLNK(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0) or os.readlink(ALIAS, dir_fd=mise_rust) != TOOLCHAIN_PATH:
                    fail("Mise toolchain selector link differs from the sealed Rust installation")
            finally:
                os.close(mise_rust)
        finally:
            os.close(mise_installs)
    finally:
        os.close(proxy_fd)
        os.close(rust_fd)
        os.close(mise_data_fd)


def prepare(tools_path, work_path, uid=UID, gid=GID, rustup_sha256=RUSTUP_SHA256, require_caller=True):
    if not sys.flags.isolated or not sys.flags.no_site:
        fail("Python must run with isolated and no-site flags")
    if require_caller and (os.geteuid(), os.getegid(), os.getgroups()) != (uid, gid, []):
        fail("home preparation requires the isolated task identity")
    tools_fd = open_abs_dir(tools_path)
    work_fd = open_abs_dir(work_path)
    try:
        identity(work_fd, uid, gid, 0o700, work_path)
        verify_toolchain(tools_fd, rustup_sha256)
        cargo_fd = os.open("cargo-seed", FLAGS, dir_fd=work_fd)
        rustup_fd = os.open("rustup-home", FLAGS, dir_fd=work_fd)
        mise_fd = os.open("mise-data", FLAGS, dir_fd=work_fd)
        identity(cargo_fd, uid, gid, 0o700, "Cargo home")
        identity(rustup_fd, uid, gid, 0o700, "Rustup home")
        identity(mise_fd, uid, gid, 0o700, "Mise data home")
        try:
            if set(os.listdir(cargo_fd)) - {"bin"}:
                fail("unexpected Cargo home entries")
            cargo_bin = ensure_dir(cargo_fd, "bin", uid, gid, 0o700, "Cargo home bin")
            try:
                if set(os.listdir(cargo_bin)) - set(PROXY_NAMES):
                    fail("unexpected Cargo bin entries")
                for name in PROXY_NAMES:
                    ensure_link(cargo_bin, name, PROXY_DIR + "/" + name, uid, gid, "Cargo bin " + name)
                if set(os.listdir(cargo_bin)) != set(PROXY_NAMES):
                    fail("Cargo proxy inventory mismatch")
            finally:
                os.close(cargo_bin)

            if set(os.listdir(rustup_fd)) - {"settings.toml", "toolchains"}:
                fail("unexpected Rustup home entries")
            toolchains = ensure_dir(rustup_fd, "toolchains", uid, gid, 0o700, "Rustup toolchains")
            try:
                if set(os.listdir(toolchains)) - {TOOLCHAIN, ALIAS}:
                    fail("unexpected Rustup toolchain entries")
                ensure_link(toolchains, TOOLCHAIN, TOOLCHAIN_PATH, uid, gid, "Rustup selected toolchain")
                ensure_link(toolchains, ALIAS, TOOLCHAIN, uid, gid, "Rustup toolchain alias")
                if set(os.listdir(toolchains)) != {TOOLCHAIN, ALIAS}:
                    fail("Rustup toolchain inventory mismatch")
            finally:
                os.close(toolchains)

            try:
                settings_fd = os.open("settings.toml", os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=rustup_fd)
            except FileNotFoundError:
                settings_fd = os.open("settings.toml", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=rustup_fd)
                if os.geteuid() == 0:
                    os.fchown(settings_fd, uid, gid)
                view = memoryview(SETTINGS)
                while view:
                    count = os.write(settings_fd, view)
                    if count <= 0:
                        fail("short Rustup settings write")
                    view = view[count:]
                os.fsync(settings_fd)
                os.close(settings_fd)
            else:
                item = os.fstat(settings_fd)
                content = os.read(settings_fd, len(SETTINGS) + 1)
                os.close(settings_fd)
                if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (uid, gid, 0o600, 1) or content != SETTINGS:
                    fail("unsafe Rustup settings file")
            verify_file(rustup_fd, "settings.toml", uid, gid, 0o600, "Rustup settings", hashlib.sha256(SETTINGS).hexdigest())
            if set(os.listdir(rustup_fd)) != {"settings.toml", "toolchains"}:
                fail("Rustup home inventory mismatch")

            installs = ensure_dir(mise_fd, "installs", uid, gid, 0o700, "Mise installs")
            try:
                if set(os.listdir(installs)) - {"rust"}:
                    fail("unexpected Mise installs")
                rust = ensure_dir(installs, "rust", uid, gid, 0o700, "Mise Rust installs")
                try:
                    if set(os.listdir(rust)) - {ALIAS}:
                        fail("unexpected Mise Rust installs")
                    ensure_link(rust, ALIAS, TOOLCHAIN_PATH, uid, gid, "Mise selected toolchain")
                    if set(os.listdir(rust)) != {ALIAS}:
                        fail("Mise Rust install inventory mismatch")
                finally:
                    os.close(rust)
            finally:
                os.close(installs)
            if set(os.listdir(mise_fd)) != {"installs"}:
                fail("Mise data home inventory mismatch")
            os.fsync(cargo_fd)
            os.fsync(rustup_fd)
            os.fsync(mise_fd)
        finally:
            os.close(cargo_fd)
            os.close(rustup_fd)
            os.close(mise_fd)
    finally:
        os.close(tools_fd)
        os.close(work_fd)
    return {
        "cargo_home": "/work/cargo-seed",
        "cargo_proxy_count": len(PROXY_NAMES),
        "mise_data_home": "/work/mise-data",
        "rustup_home": "/work/rustup-home",
        "rustup_sha256": rustup_sha256,
        "rustup_toolchain": TOOLCHAIN,
        "status": "PASS",
    }


def fixture():
    root = pathlib.Path(tempfile.mkdtemp(prefix="rustup-home-fixture-", dir="/root/.velnor-generator-preview-20261005"))
    os.chmod(root, 0o700)
    tools = root / "tools"
    work = root / "work"
    (tools / "rustup-proxy").mkdir(parents=True, mode=0o755)
    (tools / "rust-1.98.1" / "bin").mkdir(parents=True, mode=0o755)
    (tools / "mise-data" / "installs" / "rust").mkdir(parents=True, mode=0o755)
    os.chmod(tools, 0o755)
    for path in (tools / "rustup-proxy", tools / "rust-1.98.1", tools / "rust-1.98.1" / "bin", tools / "mise-data", tools / "mise-data" / "installs", tools / "mise-data" / "installs" / "rust"):
        os.chmod(path, 0o755)
    rustup = tools / "rustup-proxy" / "rustup"
    rustup.write_bytes(b"synthetic Rustup fixture\n")
    os.chmod(rustup, 0o555)
    for name in set(PROXY_NAMES) - {"rustup"}:
        (tools / "rustup-proxy" / name).symlink_to("rustup")
    for name in ("cargo", "rustc"):
        binary = tools / "rust-1.98.1" / "bin" / name
        binary.write_bytes(("synthetic " + name + " fixture\n").encode())
        os.chmod(binary, 0o755)
    (tools / "mise-data" / "installs" / "rust" / ALIAS).symlink_to(TOOLCHAIN_PATH)
    work.mkdir(mode=0o700)
    os.chmod(work, 0o700)
    for name in ("cargo-seed", "rustup-home", "mise-data"):
        (work / name).mkdir(mode=0o700)
        os.chmod(work / name, 0o700)
    if os.geteuid() == 0:
        for path in (tools, *tools.rglob("*")):
            if path.is_symlink():
                os.chown(path, 0, 0, follow_symlinks=False)
            else:
                os.chown(path, 0, 0)
        for path in (work, *work.rglob("*")):
            if path.is_symlink():
                os.chown(path, UID, GID, follow_symlinks=False)
            else:
                os.chown(path, UID, GID)
    fake_digest = hashlib.sha256(rustup.read_bytes()).hexdigest()
    try:
        result = prepare(str(tools), str(work), UID, GID, fake_digest, require_caller=False)
        repeat = prepare(str(tools), str(work), UID, GID, fake_digest, require_caller=False)
        if result != repeat:
            fail("home setup is not idempotent")
        cargo_bin = work / "cargo-seed" / "bin"
        for name in PROXY_NAMES:
            item = (cargo_bin / name).lstat()
            if not stat.S_ISLNK(item.st_mode) or (item.st_uid, item.st_gid) != (UID, GID) or os.readlink(cargo_bin / name) != PROXY_DIR + "/" + name:
                fail("fixture Cargo proxy differs: " + name)
        settings_path = work / "rustup-home" / "settings.toml"
        saved_settings = work / "rustup-home" / "settings.fixture"
        settings_path.rename(saved_settings)
        os.mkfifo(settings_path, 0o600)
        os.chown(settings_path, UID, GID)
        try:
            prepare(str(tools), str(work), UID, GID, fake_digest, require_caller=False)
        except GuardError:
            pass
        else:
            fail("FIFO Rustup settings fixture was accepted")
        settings_path.unlink()
        saved_settings.rename(settings_path)
        wrong = cargo_bin / "rustc"
        wrong.unlink()
        wrong.symlink_to("/bin/false")
        os.chown(wrong, UID, GID, follow_symlinks=False)
        try:
            prepare(str(tools), str(work), UID, GID, fake_digest, require_caller=False)
        except GuardError:
            pass
        else:
            fail("wrong preexisting proxy was accepted")
        (cargo_bin / "rustc").unlink()
        (cargo_bin / "rustc").symlink_to(PROXY_DIR + "/rustc")
        os.chown(cargo_bin / "rustc", UID, GID, follow_symlinks=False)
        (work / "cargo-seed" / "unexpected").write_text("fixture")
        try:
            prepare(str(tools), str(work), UID, GID, fake_digest, require_caller=False)
        except GuardError:
            pass
        else:
            fail("unexpected Cargo home entry was accepted")
        print(json.dumps({"fifo_settings_rejected": True, "idempotent": True, "proxy_count": len(PROXY_NAMES), "wrong_proxy_rejected": True, "extra_entry_rejected": True, "status": "PASS"}, sort_keys=True))
    finally:
        print("fixture_directory=" + str(root))


def main():
    args = sys.argv[1:]
    if args == []:
        result = prepare("/tools", "/work")
        print(json.dumps(result, sort_keys=True))
    elif args == ["fixture"]:
        fixture()
    else:
        raise SystemExit("usage: rustup-home-prepare-v1.py [fixture]")


if __name__ == "__main__":
    try:
        main()
    except GuardError as exc:
        raise SystemExit("rustup home preparation: " + str(exc))
