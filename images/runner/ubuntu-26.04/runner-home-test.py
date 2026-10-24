#!/usr/bin/env python3
"""This program does a test of Runner seed inventory and Runner home volume copy rules."""

from __future__ import annotations

import importlib.util
import os
import stat
import tempfile
from copy import deepcopy
from pathlib import Path

SCRIPT = Path(__file__).with_name("runner-home.py")
SPEC = importlib.util.spec_from_file_location("runner_home", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise SystemExit("The Runner home volume test could not load its implementation.")
runner_home = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner_home)


def write(path: Path, content: bytes, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    path.chmod(mode)


def seed_tree(root: Path) -> dict[str, object]:
    write(root / "bin" / "Runner.Listener", b"official-listener-fixture\n", 0o755)
    write(root / "bin" / "Runner.Worker", b"worker-fixture\n", 0o755)
    write(root / "externals" / "node24" / "bin" / "node", b"node-fixture\n", 0o755)
    (root / "externals" / "current").symlink_to("node24")
    entries, allocated = runner_home.inventory(str(root))
    return runner_home.identity_document(
        entries,
        allocated,
        "2.338.0",
        "https://github.com/actions/runner/releases/download/v2.338.0/actions-runner-linux-x64-2.338.0.tar.gz",
        "a" * 64,
    )


def manifest_file(path: Path, document: dict[str, object]) -> None:
    path.write_bytes(runner_home.canonical_json(document))
    path.chmod(0o444)


def initialize(seed: Path, home: Path, manifest: Path, available: int | None = None) -> None:
    runner_home.initialize(
        str(seed),
        str(home),
        str(manifest),
        expected_uid=os.getuid(),
        expected_gid=os.getgid(),
        expected_parent_uid=os.getuid(),
        expected_parent_gid=os.getgid(),
        expected_seed_uid=os.getuid(),
        expected_seed_gid=os.getgid(),
        require_mounts=False,
        require_root_manifest=False,
        available_bytes=available,
    )


def expect_failure(action: object, reason: str) -> None:
    try:
        action()  # type: ignore[operator]
    except runner_home.SeedError as error:
        if str(error) != reason:
            raise AssertionError(f"Expected the error code {reason}. Received {error}.") from error
    else:
        raise AssertionError(f"Expected the error code {reason}.")


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="velnor-runner-home-test-") as temporary:
        base = Path(os.path.realpath(temporary))
        seed = base / "seed"
        seed.mkdir()
        document = seed_tree(seed)
        manifest = base / "manifest.json"
        manifest_file(manifest, document)
        home = base / "home-parent" / "runner"
        home.mkdir(parents=True)
        (home / "_work").mkdir()
        write(home / "_work" / "kept", b"work mount fixture\n", 0o644)

        initialize(seed, home, manifest)
        copied, _ = runner_home.inventory(str(home), exclude_work=True)
        if not runner_home.compare_entries(copied, document["entries"], compare_allocation=False):
            raise AssertionError("Runner home data differs from the verified Runner seed.")
        listener = home / "bin" / "Runner.Listener"
        if not listener.is_file() or not os.access(listener, os.X_OK):
            raise AssertionError("The Runner listener was not copied as an executable file.")
        if (home / "_work" / "kept").read_bytes() != b"work mount fixture\n":
            raise AssertionError("The program changed the Work volume.")
        if stat.S_IMODE(listener.stat().st_mode) != 0o755:
            raise AssertionError("The Runner listener mode changed.")

        allocation_manifest = deepcopy(document)
        allocation_manifest["entries"][0]["allocated_bytes"] += 4096
        allocation_manifest["required_allocated_bytes"] += 4096
        allocation_path = base / "allocation-manifest.json"
        manifest_file(allocation_path, allocation_manifest)
        allocation_home = base / "allocation-home"
        allocation_home.mkdir()
        (allocation_home / "_work").mkdir()
        expect_failure(
            lambda: initialize(seed, allocation_home, allocation_path),
            "seed_manifest_mismatch",
        )

        interrupted_home = base / "interrupted-home"
        interrupted_home.mkdir()
        (interrupted_home / "_work").mkdir()
        original_inventory = runner_home.inventory
        inventory_calls = 0

        def fail_after_publish(*args: object, **kwargs: object):
            nonlocal inventory_calls
            inventory_calls += 1
            if inventory_calls == 3:
                raise runner_home.SeedError("injected_publish_failure")
            return original_inventory(*args, **kwargs)

        runner_home.inventory = fail_after_publish
        try:
            expect_failure(
                lambda: initialize(seed, interrupted_home, manifest),
                "injected_publish_failure",
            )
        finally:
            runner_home.inventory = original_inventory
        if set(os.listdir(interrupted_home)) != {"_work"}:
            raise AssertionError("Failed publication left partial Runner home data.")

        unexpected = base / "unexpected-home"
        unexpected.mkdir()
        (unexpected / "_work").mkdir()
        write(unexpected / "job-data", b"must remain untouched\n", 0o600)
        expect_failure(
            lambda: initialize(seed, unexpected, manifest),
            "home_not_fresh",
        )
        if (unexpected / "job-data").read_bytes() != b"must remain untouched\n":
            raise AssertionError("The program changed unexpected Runner home data.")

        wrong_owner = base / "wrong-owner"
        wrong_owner.mkdir()
        (wrong_owner / "_work").mkdir()
        expect_failure(
            lambda: runner_home.initialize(
                str(seed),
                str(wrong_owner),
                str(manifest),
                expected_uid=os.getuid(),
                expected_gid=os.getgid() + 1,
                expected_parent_uid=os.getuid(),
                expected_parent_gid=os.getgid(),
                expected_seed_uid=os.getuid(),
                expected_seed_gid=os.getgid(),
                require_mounts=False,
                require_root_manifest=False,
            ),
            "home_owner",
        )

        real_home = base / "real-home"
        real_home.mkdir()
        (real_home / "_work").mkdir()
        linked_home = base / "linked-home"
        linked_home.symlink_to(real_home)
        expect_failure(
            lambda: initialize(seed, linked_home, manifest),
            "home_owner",
        )

        mountinfo = base / "mountinfo"
        mountinfo.write_text(
            "36 25 0:55 / /home/runner rw - ext4 /dev/volume rw\n"
            "37 36 0:56 / /home/runner/_work rw - ext4 /dev/work rw\n",
            encoding="utf-8",
        )
        runner_home.check_home_mounts(
            "/home/runner", "/home/runner/_work", str(mountinfo)
        )
        mountinfo.write_text(
            "36 25 0:55 / /home/runner rw - ext4 /dev/volume rw\n"
            "37 36 0:56 / /home/runner/_work rw - ext4 /dev/work rw\n"
            "38 36 0:57 / /home/runner/foreign rw - ext4 /dev/foreign rw\n",
            encoding="utf-8",
        )
        expect_failure(
            lambda: runner_home.check_home_mounts(
                "/home/runner", "/home/runner/_work", str(mountinfo)
            ),
            "home_mount_layout",
        )

        low_space = base / "low-space"
        low_space.mkdir()
        (low_space / "_work").mkdir()
        required = int(document["required_allocated_bytes"])
        expect_failure(
            lambda: initialize(
                seed,
                low_space,
                manifest,
                required + runner_home.HOME_ALLOWANCE_BYTES - 1,
            ),
            "home_capacity",
        )
        if set(os.listdir(low_space)) != {"_work"}:
            raise AssertionError("Capacity rejection created Runner home files.")

        altered = base / "altered"
        altered.mkdir()
        altered_doc = seed_tree(altered)
        altered_manifest = base / "altered-manifest.json"
        manifest_file(altered_manifest, altered_doc)
        write(altered / "bin" / "Runner.Listener", b"changed listener\n", 0o755)
        altered_home = base / "altered-home"
        altered_home.mkdir()
        (altered_home / "_work").mkdir()
        expect_failure(
            lambda: initialize(altered, altered_home, altered_manifest),
            "seed_manifest_mismatch",
        )

        escaping = base / "escaping"
        escaping.mkdir()
        (escaping / "escape").symlink_to("../../outside")
        try:
            runner_home.inventory(str(escaping))
        except runner_home.SeedError as error:
            if str(error) != "seed_symlink_escape":
                raise
        else:
            raise AssertionError("A Runner seed link escaped the seed directory.")

        hardlinked = base / "hardlinked"
        hardlinked.mkdir()
        write(hardlinked / "payload", b"shared inode\n", 0o644)
        os.link(hardlinked / "payload", hardlinked / "payload-link")
        try:
            runner_home.inventory(str(hardlinked))
        except runner_home.SeedError as error:
            if str(error) != "seed_file_mode_or_link":
                raise
        else:
            raise AssertionError("A hard-linked Runner seed file passed validation.")

        special = base / "special"
        special.mkdir()
        os.mkfifo(special / "pipe")
        try:
            runner_home.inventory(str(special))
        except runner_home.SeedError as error:
            if str(error) != "seed_special_file":
                raise
        else:
            raise AssertionError("A special file passed Runner seed validation.")

    print(
        "Runner home volume tests: PASS. The tests include the Runner seed, modes, "
        "separate Work volume, ownership, mount layout, capacity, allocation "
        "identity, cleanup, and link safety."
    )


if __name__ == "__main__":
    main()
