# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import os
from pathlib import Path
import re
import secrets
import stat
import importlib.util
import json
import shutil
import struct
import subprocess
import tempfile
import time


RUNTIME_SECONDS = 200
DEADLINE_SECONDS = 225


class ScenarioError(RuntimeError):
    pass


class UnitInventory:
    def __init__(self, ledger=None):
        self.ids = set()
        self.generate = secrets.token_hex
        self.dropins = existing_dropins()
        self.ledger = ledger

    def allocate(self, size=None):
        value = self.generate(size)
        if size != 16:
            return value
        if not re.fullmatch(r"[0-9a-f]{32}", value) or value in self.ids:
            raise ScenarioError("invalid or reused scenario unit identity")
        self.check(value, unused=True)
        self.ids.add(value)
        if self.ledger is not None and os.write(self.ledger, (value + "\n").encode("ascii")) != 33:
            raise ScenarioError("scenario identity recording failed")
        return value

    def check(self, value, unused=False, allow_dropin=False):
        units = ("velnor-release-" + value + ".service", "velnor-test-blocker-" + value + ".service")
        jobs = command(["list-jobs", "--no-legend", "--plain", "--full", "--no-pager"])
        for line in jobs.splitlines():
            fields = line.split()
            if len(fields) < 2 or any(unit == fields[1] for unit in units):
                raise ScenarioError("scenario unit has a pending job")
        for unit in units:
            state = command(["show", unit, "--property=LoadState", "--property=ActiveState",
                             "--property=Job", "--property=ControlGroup"])
            values = {}
            for line in state.splitlines():
                key, separator, entry = line.partition("=")
                if not separator or key in values:
                    raise ScenarioError("invalid scenario unit state")
                values[key] = entry
            if (set(values) != {"LoadState", "ActiveState", "Job", "ControlGroup"}
                    or (unused and values["LoadState"] != "not-found")
                    or values["ActiveState"] not in ("inactive", "failed") or values["Job"]
                    or values["ControlGroup"]
                    or (not allow_dropin and os.path.lexists(Path("/run/systemd/system") / (unit + ".d")))
                    or (Path("/sys/fs/cgroup/system.slice") / unit).exists()):
                raise ScenarioError("scenario unit is not quiescent")

    def verify(self):
        self.verify_namespace()
        for value in sorted(self.ids):
            self.check(value)

    def verify_namespace(self):
        if existing_dropins() != self.dropins:
            raise ScenarioError("scenario staging was not restored")
        patterns = ("velnor-release-*.service", "velnor-test-blocker-*.service")
        jobs = command(["list-jobs", "--no-legend", "--plain", "--full", "--no-pager"])
        for line in jobs.splitlines():
            fields = line.split()
            if len(fields) < 2:
                raise ScenarioError("invalid systemd job state")
            if fields[1].startswith(("velnor-release-", "velnor-test-blocker-")):
                raise ScenarioError("scenario unit has a pending job")
        units = command(["list-units", "--all", "--no-legend", "--plain", "--no-pager", *patterns])
        for line in units.splitlines():
            fields = line.split()
            if (len(fields) < 4 or not re.fullmatch(r"velnor-(?:release|test-blocker)-[0-9a-f]{32}\.service", fields[0])
                    or fields[2] not in ("inactive", "failed")):
                raise ScenarioError("scenario namespace is not quiescent")
        root = Path("/sys/fs/cgroup/system.slice")
        if not root.is_dir() or not Path("/sys/fs/cgroup/cgroup.controllers").is_file():
            raise ScenarioError("unavailable scenario cgroup namespace")
        for pattern in patterns:
            for group in root.glob(pattern):
                data = (group / "cgroup.events").read_text(encoding="ascii")
                populated = [line for line in data.splitlines() if line.startswith("populated ")]
                if populated != ["populated 0"]:
                    raise ScenarioError("scenario namespace is populated")


def command(args):
    result = subprocess.run(["/usr/bin/systemctl", *args], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=10,
                            env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "LANG": "C"}, check=True)
    if len(result.stdout) > 1024 * 1024:
        raise ScenarioError("scenario unit state too large")
    return result.stdout.decode("utf-8", "strict")


def existing_dropins():
    root = Path("/run/systemd/system")
    return {entry.name for pattern in ("velnor-release-*.service.d", "velnor-test-blocker-*.service.d")
            for entry in root.glob(pattern)}


def start_blocker(unit, duration):
    if (not re.fullmatch(r"velnor-test-blocker-[0-9a-f]{32}\.service", unit)
            or duration not in (4, 20)):
        raise ScenarioError("invalid scenario blocker")
    directory = Path("/run/systemd/system") / (unit + ".d")
    directory.mkdir(mode=0o700)
    descriptor = os.open(directory / "deadline.conf", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="ascii") as output:
        output.write("[Unit]\nJobTimeoutSec=30s\n")
    subprocess.run(["/usr/bin/systemd-run", "--no-block", "--unit=" + unit,
                    "--property=Type=oneshot", "--property=TimeoutStartSec=30s",
                    "--property=TimeoutStopSec=5s", "/usr/bin/sleep", str(duration)],
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=5, check=True)


def finish_blocker(unit):
    match = re.fullmatch(r"velnor-test-blocker-([0-9a-f]{32})\.service", unit)
    if not match:
        raise ScenarioError("invalid scenario blocker")
    UnitInventory().check(match.group(1), allow_dropin=True)
    directory = Path("/run/systemd/system") / (unit + ".d")
    (directory / "deadline.conf").unlink()
    directory.rmdir()


def run(suite, ledger_path=None):
    if os.geteuid() != 0:
        raise ScenarioError("scenario controller requires root")
    ledger = None
    if ledger_path is not None:
        ledger = os.open(ledger_path, os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_CLOEXEC)
        metadata = os.fstat(ledger)
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_nlink != 1
                or metadata.st_mode & 0o077):
            os.close(ledger)
            raise ScenarioError("invalid scenario identity ledger")
    inventory = UnitInventory(ledger)
    original = secrets.token_hex
    try:
        secrets.token_hex = inventory.allocate
        suite()
        if ledger is None:
            inventory.verify()
        else:
            for value in sorted(inventory.ids):
                inventory.check(value)
            seal = ("!" + format(len(inventory.ids), "08d") + "\n").encode("ascii")
            if len(inventory.ids) > 256 or os.write(ledger, seal) != 10:
                raise ScenarioError("scenario identity sealing failed")
    finally:
        secrets.token_hex = original
        if ledger is not None:
            os.close(ledger)


def run_fixed():
    if os.geteuid() != 0:
        raise ScenarioError("scenario controller requires root")
    if os.path.lexists("/run/systemd/system/velnor-release-.service.d"):
        raise ScenarioError("scenario prefix staging already exists")
    source = Path(__file__).resolve().parents[1] / "src"
    RootWatch = load("scenario_watch_fixture", Path(__file__).with_name("release_watch_fixture.py")).RootWatch
    program = Path(__file__).resolve().with_name("release_manager_qualification.py")
    manager = load("scenario_manager", source / "release_manager_bus.py")
    lifecycle = load("scenario_lifecycle", source / "release_manager_lifecycle.py")

    class QualificationManager(manager.Manager):
        def _property(self, message, name, kind, value):
            if name == "CapabilityBoundingSet":
                value = 1 << 19
            if name == "StandardError":
                for limit in ("LimitFSIZE", "LimitFSIZESoft"):
                    super()._property(message, limit, "t", 1024 * 1024)
                return super()._property(message, "StandardErrorFileDescriptor", "h", diagnostics.fileno())
            return super()._property(message, name, kind, value)

    inventory = UnitInventory()
    inventory.verify_namespace()
    deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + DEADLINE_SECONDS * 1_000_000_000
    with tempfile.TemporaryFile() as diagnostics, \
            RootWatch(manager, lifecycle, source, seconds=DEADLINE_SECONDS + 60) as watch, \
            tempfile.TemporaryDirectory(prefix="velnor-scenario-controller-", dir="/run") as directory:
        helper = Path(directory) / "helper.py"
        shutil.copyfile(source / "release_supervisor_exec.py", helper)
        helper.chmod(0o555)
        ledger_path = Path(directory) / "unit-identities"
        ledger = os.open(ledger_path, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        body = json.dumps({"program": "/usr/bin/python3", "args": ["-I", "-S", "-B", str(program),
                          "--execute-scenarios"], "cwd": "/", "env": {"PATH": "/usr/bin:/bin",
                          "HOME": "/nonexistent", "LANG": "C", "VELNOR_SCENARIO_LEDGER": str(ledger_path)},
                          "stdin": ""}).encode()
        frame = struct.pack(">QQI", deadline, os.stat("/proc/self/ns/time").st_ino, len(body)) + body
        connection = QualificationManager(str(helper), RUNTIME_SECONDS, watch.authority, root_bootstrap=True)
        try:
            lifecycle.supervise(connection, frame, deadline)
            os.lseek(ledger, 0, os.SEEK_SET)
            data = os.read(ledger, 33 * 256 + 11)
            lines = data.decode("ascii").splitlines()
            if not lines or not re.fullmatch(r"![0-9]{8}", lines[-1]):
                raise ScenarioError("invalid recorded scenario identities")
            values = lines[:-1]
            if (not values or len(values) > 256 or int(lines[-1][1:]) != len(values)
                    or len(data) != 33 * len(values) + 10
                    or len(set(values)) != len(values)
                    or any(not re.fullmatch(r"[0-9a-f]{32}", value) for value in values)):
                raise ScenarioError("invalid recorded scenario identities")
            inventory.ids.update(values)
        except BaseException as error:
            diagnostics.seek(0)
            details = diagnostics.read(1024 * 1024)
            status = "truncated" if len(details) == 1024 * 1024 else "captured"
            raise ScenarioError("scenario diagnostics " + status + ":\n"
                                + details.decode("utf-8", errors="replace")) from error
        finally:
            connection.close()
            os.close(ledger)
    inventory.verify()


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
