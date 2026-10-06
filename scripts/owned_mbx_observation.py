"""Observe admitted MBX bytes; never infer native transport qualification."""

import base64
import hashlib
import os
from pathlib import Path
import platform
import re
import signal
import stat
import subprocess

from owned_tool_behavior import native_host_target
from owned_tool_source import strict_json

MAX_OUTPUT = 1024 * 1024
UNAVAILABLE = "Final MBX native ABI and source-qualified transport suite are unavailable."
SMOKE_CASES = ("owned-version", "isolated-cache-directory", "empty-cache-verification")
REPORT_FIELDS = set("schema status host source target candidate_receipt_sha256 artifact binary_sha256 "
    "version version_is_distinct environment results store_before store_after passed abi "
    "native_authority native_qualification".split())


def require(condition, message):
    if not condition:
        raise ValueError(message)


def regular(path, limit):
    named = path.lstat()
    directory = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:-1]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=directory)
            os.close(directory)
            directory = child
        descriptor = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                             dir_fd=directory)
    finally:
        os.close(directory)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1 and
                (metadata.st_dev, metadata.st_ino) == (named.st_dev, named.st_ino) and
                metadata.st_size <= limit, "bounded regular observation file required")
        raw = stream.read(limit + 1)
    require(len(raw) == metadata.st_size and len(raw) <= limit,
            "observation bytes changed or exceed bound")
    return raw, (metadata.st_dev, metadata.st_ino)


def output(path, expected_identity=None):
    raw, identity = regular(path, MAX_OUTPUT)
    require(expected_identity is None or identity == expected_identity,
            "observation output file replaced")
    return {"base64": base64.b64encode(raw).decode("ascii"),
            "sha256": hashlib.sha256(raw).hexdigest(), "size": len(raw)}, raw


def inventory(root):
    result = []
    require(not root.is_symlink(), "observation store symlink forbidden")
    count, size = 0, 0
    for directory, names, files in os.walk(root, followlinks=False):
        names.sort()
        for name in sorted(names + files):
            path = Path(directory) / name
            count += 1
            require(count <= 256, "observation store inventory exceeds bound")
            metadata = path.lstat()
            require(not stat.S_ISLNK(metadata.st_mode), "observation store symlink forbidden")
            if stat.S_ISREG(metadata.st_mode):
                data, _ = regular(path, MAX_OUTPUT)
                size += len(data)
                require(size <= 8 * MAX_OUTPUT, "observation store bytes exceed bound")
                result.append({"path": str(path.relative_to(root)), "size": len(data),
                               "sha256": hashlib.sha256(data).hexdigest()})
            else:
                require(stat.S_ISDIR(metadata.st_mode), "observation store special file forbidden")
    return result


def execute(binary, arguments, root, environment, label):
    paths = [root / (label + suffix) for suffix in (".stdout", ".stderr")]
    timed_out, code = False, None
    with paths[0].open("xb") as stdout, paths[1].open("xb") as stderr:
        identities = [(value.st_dev, value.st_ino) for value in
                      (os.fstat(stdout.fileno()), os.fstat(stderr.fileno()))]
        try:
            process = subprocess.Popen([str(binary), *arguments], cwd=root,
                env=environment, stdout=stdout, stderr=stderr, start_new_session=True)
            code = process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
    stdout, raw = output(paths[0], identities[0])
    stderr, _ = output(paths[1], identities[1])
    return {"case": label, "argv": [str(binary), *arguments], "cwd": str(root),
            "returncode": code, "timed_out": timed_out,
            "stdout": stdout, "stderr": stderr}, raw


def observe(binary, root, candidate, candidate_receipt_sha256):
    host = {"system": platform.system(), "machine": platform.machine()}
    require(candidate["tool"] == "mbx" and
            candidate["target"] == native_host_target(host), "MBX observation host/tool mismatch")
    require(binary.is_absolute() and binary == binary.resolve(strict=True) and
            binary.is_file() and not binary.is_symlink(), "canonical regular MBX required")
    data, binary_identity = regular(binary, 512 * MAX_OUTPUT)
    digest = hashlib.sha256(data).hexdigest()
    require(digest == candidate["artifact"]["binary_sha256"], "MBX observation binary mismatch")
    require(root.is_absolute() and root == root.resolve(strict=True), "canonical observation cwd required")
    metadata = root.stat()
    require(metadata.st_uid == os.getuid() and stat.S_IMODE(metadata.st_mode) == 0o700,
            "private owned observation root required")
    require(not any(path.exists() or path.is_symlink() for path in
                    (root / "home", root / "cache")), "fresh observation home/cache required")
    for directory in (root, *root.parents):
        require(not (directory / ".mbx.toml").exists() and
                not (directory / ".mbx.toml").is_symlink(), "ambient MBX workspace policy present")
    home = root / "home"
    home.mkdir(mode=0o700)
    require(home.stat().st_uid == os.getuid() and stat.S_IMODE(home.stat().st_mode) == 0o700,
            "private owned observation home required")
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"), "XDG_CACHE_HOME": str(home / ".cache"),
        "XDG_DATA_HOME": str(home / ".local/share"), "MBX_CACHE_DIR": str(root / "cache"),
        "MBX_GC_AUTO": "0", "MBX_DISPLAY": "plain", "MBX_SUMMARY": "off",
        "LANG": "C", "LC_ALL": "C", "TZ": "UTC"}
    results = []
    before = inventory(root / "cache")
    version_case, raw = execute(binary, ["--version"], root, environment, "owned-version")
    results.append(version_case)
    version = raw.decode("utf-8", errors="replace").strip()
    versions = re.findall(r"(?<![A-Za-z0-9.-])[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version)
    distinct = versions == [candidate["version"]] and "DEBUG" not in version
    version_case["observation"] = ("matched" if version_case["returncode"] == 0 and
        distinct and version == candidate["version_banner"].strip() else "mismatched")
    # A mismatched executable identity never reaches another MBX command.
    if version_case["observation"] == "matched":
        directory_case, raw = execute(binary, ["cache", "dir", "--json"], root,
                                      environment, "isolated-cache-directory")
        results.append(directory_case)
        try:
            store = strict_json(raw)
            location = Path(store["store"])
            observed = (set(store) == {"version", "store"} and type(store["version"]) is int
                and store["version"] == 1 and location == root / "cache/actions")
        except (ValueError, KeyError, TypeError):
            observed = False
        directory_case["observation"] = ("matched" if directory_case["returncode"] == 0
                                           and observed else "mismatched")
        if directory_case["observation"] == "matched":
            inventory(root / "cache")
            verify_case, raw = execute(binary, ["cache", "verify"], root,
                                       environment, "empty-cache-verification")
            verify_case["observation"] = ("matched" if verify_case["returncode"] == 0 and
                raw.strip() == b"verified 0 objects and 0 action results" else "mismatched")
            results.append(verify_case)
    data, final_identity = regular(binary, 512 * MAX_OUTPUT)
    require(final_identity == binary_identity and hashlib.sha256(data).hexdigest() == digest,
            "MBX binary changed during observation")
    return {"schema": 1, "status": "OBSERVED_MBX_SMOKE_ONLY", "host": host,
        "source": candidate["source"], "target": candidate["target"],
        "candidate_receipt_sha256": candidate_receipt_sha256, "artifact": candidate["artifact"],
        "binary_sha256": digest, "version": version, "version_is_distinct": distinct,
        "environment": environment, "results": results,
        "store_before": before, "store_after": inventory(root / "cache"), "passed": False,
        "abi": None, "native_authority": None,
        "native_qualification": {"status": "unavailable", "reason": UNAVAILABLE}}


def smoke_matches(report):
    results = report.get("results")
    return (set(report) == REPORT_FIELDS and type(report.get("schema")) is int and
        report["schema"] == 1 and report.get("version_is_distinct") is True and
        report.get("native_qualification") == {"status": "unavailable", "reason": UNAVAILABLE} and
        report.get("store_before") == [] and report.get("store_after") == [] and
        isinstance(results, list) and len(results) == len(SMOKE_CASES) and
        [case.get("case") for case in results if isinstance(case, dict)] == list(SMOKE_CASES) and
        all(type(case.get("returncode")) is int and case["returncode"] == 0 and
            case.get("timed_out") is False and case.get("observation") == "matched"
            for case in results))
