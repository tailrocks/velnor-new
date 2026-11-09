# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import copy
import json
import pathlib
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import driver

fixture = json.loads(pathlib.Path(__file__).with_name("worker_listing_fixture.json").read_text())
with tempfile.TemporaryDirectory() as temporary:
    target = pathlib.Path(temporary)
    binaries = fixture["binaries"]
    cargo = fixture["cargo"]
    record = binaries["rust-binaries"]["velnor-actions-orchestrator"]
    executable = target / pathlib.PurePosixPath(record["binary-path"]).relative_to("/target")
    executable.parent.mkdir(parents=True)
    executable.write_bytes(b"actual fixture executable")
    executable.chmod(0o700)

    def check(b=binaries, c=cargo):
        (target / "fixture-worker-binaries.json").write_text(json.dumps(b))
        (target / "fixture-cargo.json").write_text(json.dumps(c))
        return driver.worker_executable(target, "debug", "aarch64-unknown-linux-gnu")

    assert check() == record["binary-path"]

    def refuse(b=binaries, c=cargo):
        try:
            check(b, c)
        except (RuntimeError, ValueError, OSError):
            return
        raise AssertionError("invalid worker custody was accepted")

    for field, value in (("binary-id", "other"), ("binary-name", "other"),
                         ("package-id", "other"), ("kind", "test"),
                         ("build-platform", "host"), ("binary-path", "relative"),
                         ("binary-path", "/target/debug/deps/../escape"),
                         ("binary-path", "/target/debug/deps/nested/worker"),
                         ("binary-path", "/target/release/deps/worker")):
        changed = copy.deepcopy(binaries)
        changed["rust-binaries"]["velnor-actions-orchestrator"][field] = value
        refuse(changed)
    for field, value in (("workspace_members", []), ("packages", []),
                         ("packages", cargo["packages"] * 2)):
        changed = copy.deepcopy(cargo)
        changed[field] = value
        refuse(c=changed)
    changed = copy.deepcopy(binaries)
    changed["rust-binaries"]["duplicate"] = record
    refuse(changed)
    changed = copy.deepcopy(binaries)
    changed["rust-build-meta"]["platforms"]["host"]["platform"]["triple"] = "wrong"
    refuse(changed)
    changed = copy.deepcopy(binaries)
    changed["rust-build-meta"]["target-platforms"][0]["triple"] = "wrong"
    refuse(changed)
    for field, value in (("rust-binaries", []), ("rust-binaries", None), ("rust-build-meta", {})):
        changed = copy.deepcopy(binaries)
        changed[field] = value
        refuse(changed)
    for raw in (b"{", b"null", b'{"rust-binaries":{},"rust-binaries":{}}', b" " * (1024 * 1024 + 1)):
        check()
        (target / "fixture-worker-binaries.json").write_bytes(raw)
        try:
            driver.worker_executable(target, "debug", "aarch64-unknown-linux-gnu")
        except (RuntimeError, ValueError, OSError):
            pass
        else:
            raise AssertionError("invalid worker metadata was accepted")
    executable.chmod(0o600)
    refuse()
    executable.unlink()
    executable.symlink_to("/usr/bin/true")
    refuse()
    executable.unlink()
    executable.mkdir()
    refuse()
    executable.rmdir()
    executable.parent.rmdir()
    external = target / "external"
    external.mkdir()
    executable.parent.symlink_to(external, target_is_directory=True)
    refuse()
    executable.parent.unlink()
    executable.parent.mkdir()
    executable.write_bytes(b"actual fixture executable")
    executable.chmod(0o700)
    assert check() == record["binary-path"]
    executable.unlink()
    refuse()
