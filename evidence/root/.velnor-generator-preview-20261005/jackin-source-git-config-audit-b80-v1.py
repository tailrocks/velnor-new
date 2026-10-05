#!/usr/bin/python3
"""Reject Git configuration that can execute filters or load external config."""
import hashlib
import os
import pathlib
import stat
import subprocess
import sys

GIT = "/usr/bin/git"
ORIGIN = "https://github.com/jackin-project/jackin.git"
MAX_CONFIG = 65536


def fail(message):
    raise SystemExit("git config audit: " + message)


def safe_config_pairs(config_fd):
    env = {
        "PATH": "/usr/bin:/bin",
        "HOME": "/nonexistent",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_SYSTEM": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_NO_LAZY_FETCH": "1",
    }
    config_fd_path = "/proc/self/fd/" + str(config_fd)
    result = subprocess.run(
        [GIT, "config", "--file", config_fd_path, "--no-includes", "--null", "--list"],
        pass_fds=(config_fd,),
        env=env,
        input=None,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=5,
    )
    if result.returncode:
        fail("Git could not parse the isolated local config")
    data = result.stdout
    if not data.endswith(b"\0"):
        fail("Git config output is incomplete")
    items = data[:-1].split(b"\0")
    pairs = []
    for item in items:
        key_raw, separator, value_raw = item.partition(b"\n")
        if not separator:
            fail("Git config output has an incomplete key/value pair")
        try:
            key = key_raw.decode("ascii").lower()
            value = value_raw.decode("utf-8")
        except UnicodeDecodeError:
            fail("Git config contains invalid text")
        pairs.append((key, value))
    return pairs


def permitted(key, value):
    fixed = {
        "core.repositoryformatversion": {"0", "1"},
        "core.filemode": {"true"},
        "core.bare": {"false"},
        "core.logallrefupdates": {"true"},
        "core.hookspath": {"/dev/null"},
        "remote.origin.url": {ORIGIN},
        "remote.origin.tagopt": {"--no-tags"},
        "remote.origin.fetch": {
            "+refs/heads/main:refs/remotes/origin/main",
            "+refs/heads/*:refs/remotes/origin/*",
        },
        "remote.origin.promisor": {"true"},
        "remote.origin.partialclonefilter": {"blob:none"},
        "credential.helper": {""},
        "gc.auto": {"0"},
        "maintenance.auto": {"false"},
    }
    if key in fixed:
        return value in fixed[key]
    if key.startswith("branch.") and key.endswith(".remote"):
        branch = key[len("branch.") : -len(".remote")]
        return bool(branch) and value == "origin"
    if key.startswith("branch.") and key.endswith(".merge"):
        branch = key[len("branch.") : -len(".merge")]
        return bool(branch) and value.startswith("refs/heads/") and ".." not in value
    return False


def main():
    if len(sys.argv) != 2:
        fail("usage: preseed-preview-git-config-audit.py SOURCE")
    source = pathlib.Path(sys.argv[1])
    if not source.is_absolute() or source.is_symlink():
        fail("source path must be absolute and not a symlink")
    source_st = source.lstat()
    if not stat.S_ISDIR(source_st.st_mode) or source_st.st_uid != 0 or source_st.st_gid != 0:
        fail("source directory is not task-owned")
    if source_st.st_mode & 0o022:
        fail("source directory is writable by group or others")
    gitdir = source / ".git"
    if gitdir.is_symlink() or not gitdir.is_dir():
        fail("source must contain a direct Git metadata directory")
    gitdir_st = gitdir.lstat()
    if gitdir_st.st_uid != 0 or gitdir_st.st_gid != 0 or gitdir_st.st_mode & 0o022:
        fail("Git metadata directory is not task-owned")
    def path_state(relative):
        current = gitdir
        parts = pathlib.PurePosixPath(relative).parts
        for index, part in enumerate(parts):
            current = current / part
            try:
                st = current.lstat()
            except FileNotFoundError:
                return None
            if stat.S_ISLNK(st.st_mode):
                fail("symlink in local Git metadata path")
            if index + 1 < len(parts) and not stat.S_ISDIR(st.st_mode):
                fail("non-directory in local Git metadata path")
        return st

    for forbidden in ("config.worktree", "commondir", "objects/info/alternates", "objects/info/http-alternates", "info/grafts", "worktrees"):
        if path_state(forbidden) is not None:
            fail("alternate Git configuration or object source is present")
    replace_state = path_state("refs/replace")
    if replace_state is not None:
        if not stat.S_ISDIR(replace_state.st_mode):
            fail("replacement refs are present")
        replace_dir = gitdir / "refs" / "replace"
        with os.scandir(replace_dir) as entries:
            if any(entries):
                fail("replacement refs are present")
    config_path = gitdir / "config"
    config_fd = os.open(config_path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        before = os.fstat(config_fd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > MAX_CONFIG:
            fail("unsafe local Git config file")
        if before.st_uid != 0 or before.st_gid != 0 or before.st_mode & 0o022:
            fail("unexpected local Git config ownership or permissions")
        config_data = bytearray()
        while len(config_data) <= MAX_CONFIG:
            chunk = os.read(config_fd, min(4096, MAX_CONFIG + 1 - len(config_data)))
            if not chunk:
                break
            config_data.extend(chunk)
        if len(config_data) > MAX_CONFIG:
            fail("local Git config exceeds limit")
        os.lseek(config_fd, 0, os.SEEK_SET)
        pairs = safe_config_pairs(config_fd)
        after = os.fstat(config_fd)
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
            after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns
        ):
            fail("local Git config changed during audit")
    finally:
        os.close(config_fd)
    config_data = bytes(config_data)
    allowed_fetch = 0
    for key, value in pairs:
        if not permitted(key, value):
            fail("unapproved local Git config key or value: " + key)
        if key == "remote.origin.fetch":
            allowed_fetch += 1
    if allowed_fetch > 1:
        fail("multiple origin fetch specifications are not allowed")
    if not any(key == "remote.origin.url" and value == ORIGIN for key, value in pairs):
        fail("canonical Jackin repository origin is absent")
    print("PASS: local Git config allowlist; " + str(len(pairs)) + " entries; sha256=" + hashlib.sha256(config_data).hexdigest())


if __name__ == "__main__":
    main()
