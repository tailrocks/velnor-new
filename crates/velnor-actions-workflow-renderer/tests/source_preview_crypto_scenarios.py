# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
fixture = root / "crates/velnor-actions-workflow-renderer/tests/fixtures/source_preview_signature"
sys.path.insert(0, str(root / "crates/velnor-actions-workflow-renderer/src/native_helper"))
spec = importlib.util.spec_from_file_location("source_preview_stage_verify", sys.path[0] + "/source_preview_stage_verify.py")
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def gh_binary():
    explicit = os.environ.get("VELNOR_GH_BINARY")
    if explicit:
        return Path(explicit).resolve()
    allowed = stage.GH_SHA256[(platform.system(), platform.machine())]
    for version in ("2.103.2", "2.103.3"):
        result = subprocess.run(["mise", "where", f"gh@{version}"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, check=False)
        if result.returncode == 0:
            install = Path(result.stdout.strip())
            matches = [path for path in install.rglob("gh") if path.is_file() and not path.is_symlink() and digest(path) in allowed]
            if len(matches) == 1:
                return matches[0].resolve()
    raise AssertionError("exact pinned gh executable unavailable")


class SockFilter(ctypes.Structure):
    _fields_ = [("code", ctypes.c_ushort), ("jt", ctypes.c_ubyte), ("jf", ctypes.c_ubyte), ("k", ctypes.c_uint32)]


class SockFprog(ctypes.Structure):
    _fields_ = [("len", ctypes.c_ushort), ("filter", ctypes.POINTER(SockFilter))]


def deny_linux_connect():
    syscalls = {"x86_64": (0xC000003E, 41, 42), "aarch64": (0xC00000B7, 198, 203)}.get(platform.machine())
    if syscalls is None:
        raise RuntimeError("unsupported Linux architecture")
    arch, socket, connect = syscalls
    rules = (SockFilter * 9)(SockFilter(0x20, 0, 0, 4), SockFilter(0x15, 1, 0, arch), SockFilter(0x06, 0, 0, 0), SockFilter(0x20, 0, 0, 0), SockFilter(0x45, 2, 0, 0x40000000), SockFilter(0x15, 1, 0, socket), SockFilter(0x15, 0, 1, connect), SockFilter(0x06, 0, 0, 0x00050001), SockFilter(0x06, 0, 0, 0x7fff0000))
    program = SockFprog(len(rules), rules)
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(38, 1, 0, 0, 0) != 0 or libc.prctl(22, 2, ctypes.byref(program)) != 0:
        raise OSError(ctypes.get_errno(), "network filter unavailable")


def run_verify(gh, subject, bundle, roots, workflow):
    command = [str(gh), "attestation", "verify", str(subject), "--bundle", str(bundle), "--custom-trusted-root", str(roots), "--repo", "tailrocks/velnor-new", "--signer-workflow", workflow, "--signer-digest", "c622bfb187556afb8d3ff3ca917a719e4bb7f32a", "--source-digest", "c622bfb187556afb8d3ff3ca917a719e4bb7f32a", "--source-ref", "refs/heads/main", "--format", "json"]
    options = {}
    if platform.system() == "Darwin":
        command = ["/usr/bin/sandbox-exec", "-p", "(version 1)(allow default)(deny network*)", *command]
    elif platform.system() == "Linux":
        options["preexec_fn"] = deny_linux_connect
    else:
        raise RuntimeError("unsupported verifier platform")
    environment = {key: os.environ[key] for key in ("HOME", "LANG", "LC_ALL", "TMPDIR") if key in os.environ}
    environment.update({"PATH": "/usr/bin:/bin", "GH_PROMPT_DISABLED": "1"})
    return subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment, timeout=30, **options)


gh = gh_binary()
stage.verify_gh_binary(str(gh))
sidecar = fixture / "velnor-actions-0.1.74-aarch64-apple-darwin.sha256"
bundle = fixture / "bundle.intoto.jsonl"
roots = fixture / "trusted-root.jsonl"
assert digest(sidecar) == "43ad35775a05136a4d2dc0d5e698c9b57ae2db04b8b43e90611dfde3b86b6c7c"
assert digest(bundle) == "f77425db406cb74b5f56dfb92ec43084d898a025e5df07e6cb7629b4e1709cde"
assert digest(roots) == "65ca537f6ed8a47fd0e560c421baa1f6c1efb8b25fc200d8c5c02c0e92eb2b9c"
verified = run_verify(gh, sidecar, bundle, roots, "tailrocks/velnor-new/.github/workflows/product-release-generator.yml")
assert verified.returncode == 0, verified.stderr.decode(errors="replace")
result = json.loads(verified.stdout)
assert len(result) == 1
statement = result[0]["verificationResult"]["statement"]
assert any(subject["name"] == sidecar.name and subject["digest"] == {"sha256": digest(sidecar)} for subject in statement["subject"])
certificate = result[0]["verificationResult"]["signature"]["certificate"]
assert not stage.certificate_matches(certificate, "c622bfb187556afb8d3ff3ca917a719e4bb7f32a", ".github/workflows/product-release-generator.yml", 41979375157, 1, {"repository": stage.bootstrap.REPOSITORY, "path": stage.AUTHORITY_WORKFLOW, "ref": stage.bootstrap.REPOSITORY + "/" + stage.AUTHORITY_WORKFLOW + "@" + "d" * 40, "sha": "d" * 40})
with tempfile.TemporaryDirectory() as temp:
    altered = Path(temp) / sidecar.name
    altered.write_bytes(sidecar.read_bytes() + b"x")
    assert run_verify(gh, altered, bundle, roots, "tailrocks/velnor-new/.github/workflows/product-release-generator.yml").returncode != 0
    assert run_verify(gh, sidecar, bundle, roots, "tailrocks/velnor-new/.github/workflows/not-the-signer.yml").returncode != 0
