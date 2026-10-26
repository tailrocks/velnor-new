# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import hmac
import os
import stat
import sys

STAGED = 0
COLD_MISS = 10
REJECTED = 1

PAYLOAD_DIR_ENV = "VELNOR_BOOTSTRAP_PAYLOAD_DIR"
MEMBER_ENV = "VELNOR_BOOTSTRAP_MEMBER"
EXPECTED_SHA_ENV = "VELNOR_BOOTSTRAP_EXPECTED_SHA256"
STAGED_PATH_ENV = "VELNOR_BOOTSTRAP_STAGED_PATH"
MAX_BYTES_ENV = "VELNOR_BOOTSTRAP_MAX_BYTES"
EXPECTED_KEY_ENV = "VELNOR_BOOTSTRAP_EXPECTED_KEY"
MATCHED_KEY_ENV = "VELNOR_BOOTSTRAP_MATCHED_KEY"
SHADOW_ENV = "VELNOR_BOOTSTRAP_SHADOW_STATE"

CLEAR_SHADOW = "clear"
MAX_MEMBER_LEN = 256
MAX_ENTRIES = 65536
MAX_DEPTH = 32
MAX_LIMIT = 2147483647
CHUNK = 65536


class PayloadError(Exception):
    pass


def lower_hex(text, size):
    if len(text) != size:
        return False
    for byte in text.encode("utf-8"):
        digit = 48 <= byte and byte <= 57
        lower = 97 <= byte and byte <= 102
        if not digit and not lower:
            return False
    return True


def clean(text):
    return chr(0) not in text and chr(10) not in text and chr(13) not in text


def absolute(path):
    return path.startswith("/") and clean(path)


def part_ok(part):
    if not part or part == "." or part == "..":
        return False
    for byte in part.encode("utf-8"):
        alnum = 48 <= byte and byte <= 57
        upper = 65 <= byte and byte <= 90
        lower = 97 <= byte and byte <= 122
        if not alnum and not upper and not lower and byte not in (45, 46, 95):
            return False
    return True


def member_ok(member):
    if not member or len(member) > MAX_MEMBER_LEN or member.startswith("/"):
        return False
    return all(part_ok(part) for part in member.split("/"))


def limit_ok(text):
    if not text or not text.isascii() or not text.isdigit():
        return None
    try:
        value = int(text, 10)
    except ValueError:
        return None
    if value < 1 or value > MAX_LIMIT:
        return None
    return value


def scan(payload, member):
    wanted_base = member.rsplit("/", 1)[-1]
    regular = []
    stack = [payload]
    total = 0
    while stack:
        node = stack.pop()
        try:
            entries = list(os.scandir(node))
        except OSError:
            raise PayloadError("payload_unreadable")
        for entry in entries:
            total += 1
            if total > MAX_ENTRIES:
                raise PayloadError("payload_unbounded")
            try:
                linked = entry.is_symlink()
            except OSError:
                raise PayloadError("payload_unreadable")
            if linked:
                raise PayloadError("payload_link")
            try:
                info = entry.stat(follow_symlinks=False)
            except OSError:
                raise PayloadError("payload_unreadable")
            mode = info.st_mode
            if stat.S_ISDIR(mode):
                stack.append(entry.path)
            elif stat.S_ISREG(mode):
                rel = entry.path[len(payload) + 1 :]
                regular.append((rel, info.st_dev, info.st_ino, entry.path))
            else:
                raise PayloadError("payload_special")
    candidate = [item for item in regular if item[0] == member]
    if not candidate:
        return None, len(regular)
    if len(candidate) > 1:
        raise PayloadError("duplicate_member")
    picked = candidate[0]
    for item in regular:
        if item[0] != member:
            if (item[1], item[2]) == (picked[1], picked[2]):
                raise PayloadError("duplicate_member")
            if item[0].rsplit("/", 1)[-1] == wanted_base:
                raise PayloadError("duplicate_member")
    return picked[3], len(regular)


def ensure_real_dir(path):
    depth = 0
    for part in path.split("/"):
        if not part:
            continue
        if not part_ok(part):
            raise PayloadError("staged_parent")
        depth += 1
        if depth > MAX_DEPTH:
            raise PayloadError("staged_parent")
    try:
        os.makedirs(path, mode=0o755, exist_ok=True)
    except OSError:
        raise PayloadError("stage_failed")
    try:
        info = os.lstat(path)
    except OSError:
        raise PayloadError("stage_failed")
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise PayloadError("staged_parent")


def stage_verified(source, staged, expected_sha):
    parent = os.path.dirname(staged)
    if not parent or not absolute(parent):
        raise PayloadError("staged_parent")
    ensure_real_dir(parent)
    base = staged.rsplit("/", 1)[-1]
    if not part_ok(base):
        raise PayloadError("staged_name")
    tmp = staged + ".tmp"
    if os.path.islink(tmp):
        raise PayloadError("stage_failed")
    digest = hashlib.sha256()
    size = 0
    try:
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
    except OSError:
        raise PayloadError("stage_failed")
    try:
        with os.fdopen(fd, "wb") as handle:
            with open(source, "rb") as origin:
                while True:
                    chunk = origin.read(CHUNK)
                    if not chunk:
                        break
                    handle.write(chunk)
                    digest.update(chunk)
                    size += len(chunk)
    except OSError:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise PayloadError("member_unreadable")
    if not hmac.compare_digest(digest.hexdigest(), expected_sha):
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise PayloadError("digest_mismatch")
    try:
        os.chmod(tmp, 0o755)
        os.rename(tmp, staged)
    except OSError:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise PayloadError("stage_failed")
    return size


def run():
    payload = os.environ.get(PAYLOAD_DIR_ENV, "").rstrip("/")
    member = os.environ.get(MEMBER_ENV, "")
    expected_sha = os.environ.get(EXPECTED_SHA_ENV, "")
    staged = os.environ.get(STAGED_PATH_ENV, "")
    limit = limit_ok(os.environ.get(MAX_BYTES_ENV, ""))
    if not payload or not absolute(payload) or ".." in payload.split("/"):
        sys.stderr.write("reject:payload_dir\n")
        return REJECTED
    if not member_ok(member):
        sys.stderr.write("reject:member\n")
        return REJECTED
    if not lower_hex(expected_sha, 64):
        sys.stderr.write("reject:expected_sha256\n")
        return REJECTED
    if not staged or not clean(staged):
        sys.stderr.write("reject:staged_path\n")
        return REJECTED
    if limit is None:
        sys.stderr.write("reject:max_bytes\n")
        return REJECTED
    expected_key = os.environ.get(EXPECTED_KEY_ENV, "")
    matched_key = os.environ.get(MATCHED_KEY_ENV, "")
    if expected_key or matched_key:
        if not expected_key or matched_key != expected_key:
            sys.stderr.write("cold_miss:entry\n")
            return COLD_MISS
    shadow = os.environ.get(SHADOW_ENV, "")
    if shadow and shadow != CLEAR_SHADOW:
        sys.stderr.write("cold_miss:shadow\n")
        return COLD_MISS
    try:
        top = os.lstat(payload)
    except FileNotFoundError:
        sys.stderr.write("cold_miss:payload_absent\n")
        return COLD_MISS
    except OSError:
        sys.stderr.write("reject:payload_unreadable\n")
        return REJECTED
    if stat.S_ISLNK(top.st_mode):
        sys.stderr.write("reject:payload_link\n")
        return REJECTED
    if not stat.S_ISDIR(top.st_mode):
        sys.stderr.write("reject:payload_shape\n")
        return REJECTED
    try:
        found = scan(payload, member)
    except PayloadError as err:
        sys.stderr.write("reject:" + str(err) + "\n")
        return REJECTED
    source, total = found
    if source is None:
        sys.stderr.write("cold_miss:member_absent\n")
        return COLD_MISS
    try:
        size = os.path.getsize(source)
    except OSError:
        sys.stderr.write("reject:member_unreadable\n")
        return REJECTED
    if size > limit:
        sys.stderr.write("reject:member_oversized\n")
        return REJECTED
    try:
        staged_size = stage_verified(source, staged, expected_sha)
    except PayloadError as err:
        sys.stderr.write("reject:" + str(err) + "\n")
        return REJECTED
    sys.stdout.write("bootstrapped=" + staged + "\n")
    sys.stdout.write("member_bytes=" + str(staged_size) + "\n")
    sys.stdout.write("payload_files=" + str(total) + "\n")
    return STAGED


if __name__ == "__main__":
    sys.exit(run())
