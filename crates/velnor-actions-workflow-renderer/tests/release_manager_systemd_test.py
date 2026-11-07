# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import hashlib
import builtins
import errno
import importlib.util
import io
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_manager_scenario_controller import UnitInventory, finish_blocker, start_blocker
from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans


def load(name):
    spec = importlib.util.spec_from_file_location(name, root / "src" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bus = load("release_manager_bus")
lifecycle = load("release_manager_lifecycle")


def deadline(seconds):
    return time.clock_gettime_ns(time.CLOCK_BOOTTIME) + seconds * 1_000_000_000


class RealManagerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if os.geteuid() != 0:
            raise RuntimeError("systemd fixture requires root")
        cls.directory = Path(tempfile.mkdtemp(prefix="velnor-supervisor-", dir="/run"))
        cls.directory.chmod(0o755)
        cls.helper = cls.directory / "helper.py"
        shutil.copyfile(root / "src/release_supervisor_exec.py", cls.helper)
        cls.helper.chmod(0o555)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.directory)

    def setUp(self):
        existing = snapshot_roots()
        self.addCleanup(cleanup_orphans, existing)
        self.watch = RootWatch(bus, lifecycle, root / "src")
        self.addCleanup(self.watch.close)

    def test_dependent_cleanup_policy_rejects_effective_overrides(self):
        shared = Path('/run/systemd/system/velnor-release-.service.d')
        shared.mkdir(mode=0o700)
        override = shared / 'cleanup-policy.conf'
        try:
            for section, setting in (
                    ('Service', 'KillSignal=SIGTERM'),
                    ('Service', 'KillMode=mixed'),
                    ('Unit', 'CollectMode=inactive')):
                with self.subTest(setting=setting):
                    policy = '[' + section + ']\n' + setting + '\n'
                    override.write_text(policy, encoding='ascii')
                    ident = secrets.token_hex(16)
                    unit = 'velnor-release-' + ident + '.service'
                    manager = bus.Manager(str(self.helper), 25, self.watch.authority, root_bootstrap=True)
                    reader, writer = os.pipe()
                    try:
                        end = deadline(60)
                        manager.subscribe(end)
                        with self.assertRaises(bus.StartRejected) as rejected:
                            manager.start(unit, reader, end)
                        self.assertIn('unreviewed applicable manager policy', str(rejected.exception.__cause__))
                        self.assertFalse((Path('/run/systemd/system') / (unit + '.d')).exists())
                        UnitInventory().check(ident, unused=True)
                        self.assertEqual(override.read_text(encoding='ascii'), policy)
                    finally:
                        os.close(reader)
                        os.close(writer)
                        manager.close()
        finally:
            override.unlink(missing_ok=True)
            shared.rmdir()

    def test_refused_policy_never_delivers_frame_and_watch_drains(self):
        shared = Path('/run/systemd/system/velnor-release-.service.d')
        for section, setting in (
                ('Service', 'KillSignal=SIGTERM'),
                ('Service', 'KillMode=mixed'),
                ('Unit', 'CollectMode=inactive')):
            with self.subTest(setting=setting):
                watch = RootWatch(bus, lifecycle, root / 'src')
                manager = bus.Manager(str(self.helper), 40, watch.authority, root_bootstrap=True)
                ident = secrets.token_hex(16)
                unit = 'velnor-release-' + ident + '.service'
                marker = self.directory / ('executed-' + secrets.token_hex(8))
                end = deadline(30)
                body = json.dumps({'program': '/usr/bin/python3',
                    'args': ['-c', f"open({str(marker)!r}, 'w').write('executed')"],
                    'cwd': str(self.directory), 'env': {'PATH': '/usr/bin:/bin',
                    'HOME': str(self.directory)}, 'stdin': ''}, separators=(',', ':')).encode()
                frame = struct.pack('>QQI', end, os.stat('/proc/self/ns/time').st_ino, len(body)) + body
                shared.mkdir(mode=0o700)
                override = shared / 'cleanup-policy.conf'
                policy = '[' + section + ']\n' + setting + '\n'
                try:
                    override.write_text(policy, encoding='ascii')
                    with patch.object(lifecycle.secrets, 'token_hex', return_value=ident):
                        with self.assertRaisesRegex(lifecycle.SupervisorError, '^unit cleanup uncertain$'):
                            lifecycle.supervise(manager, frame, end)
                    self.assertFalse(marker.exists())
                    self.assertFalse((Path('/run/systemd/system') / (unit + '.d')).exists())
                    UnitInventory().check(ident, unused=True)
                    self.assertEqual(override.read_text(encoding='ascii'), policy)
                    watch.close()
                    self.assertFalse(marker.exists())
                    self.assertEqual(override.read_text(encoding='ascii'), policy)
                finally:
                    manager.close()
                    override.unlink(missing_ok=True)
                    shared.rmdir()
                    if not watch.watch.closed:
                        watch.close()

    def test_failed_dependent_unit_is_observed_before_release_then_collected(self):
        unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
        manager = bus.Manager(str(self.helper), 10, self.watch.authority, root_bootstrap=True)
        reader, writer = os.pipe()
        end = deadline(20)
        try:
            owner = manager.subscribe(end)
            lifecycle.wait_job(manager, unit, manager.start(unit, reader, end), end)
            initial = lifecycle.state(manager, owner, unit, end)
            self.assertTrue(initial['populated'])
            lifecycle.wait_job(manager, unit, manager.stop(unit, end), end)
            stopped = lifecycle.state(manager, owner, unit, end)
            self.assertEqual(stopped['invocation'], initial['invocation'])
            self.assertEqual(stopped['active'], 'failed')
            self.assertEqual(stopped['result'], 'signal')
            self.assertIsNone(stopped['job'])
            self.assertFalse(stopped['populated'])
            manager.release(unit, end)
            while True:
                result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--property=LoadState', '--value'],
                                        capture_output=True, text=True, check=True, timeout=5)
                if result.stdout.strip() == 'not-found':
                    break
                remaining = end - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
                if remaining <= 0:
                    self.fail('failed dependent unit was not collected')
                time.sleep(min(0.01, remaining / 1_000_000_000))
        finally:
            os.close(reader)
            os.close(writer)
            manager.close()
            subprocess.run(['/usr/bin/systemctl', 'reset-failed', unit],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False, timeout=5)

    def test_accepted_remote_write_survives_interrupted_response(self):
        unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
        manager = bus.Manager(str(self.helper), 10, self.watch.authority, root_bootstrap=True)
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        listener.settimeout(10)
        reader, writer = os.pipe()
        peer = None
        end = deadline(35)
        payload = b'accepted-upload'
        command = ("import socket,sys; s=socket.create_connection(('127.0.0.1',int(sys.argv[1]))); "
                   "s.sendall(b'POST /upload HTTP/1.1\\r\\nContent-Length: 15\\r\\n\\r\\naccepted-upload'); "
                   "s.recv(1)")
        body = json.dumps({'program': '/usr/bin/python3', 'args': ['-I', '-S', '-c', command,
                           str(listener.getsockname()[1])], 'cwd': '/', 'env': {}, 'stdin': ''}).encode()
        frame = struct.pack('>QQI', end, os.stat('/proc/self/ns/time').st_ino, len(body)) + body
        try:
            owner = manager.subscribe(end)
            job = manager.start(unit, reader, end)
            os.close(reader)
            reader = None
            lifecycle.wait_job(manager, unit, job, end)
            invocation = lifecycle.state(manager, owner, unit, end)['invocation']
            lifecycle.write_frame(writer, frame, end)
            os.close(writer)
            writer = None
            peer, _ = listener.accept()
            peer.settimeout(10)
            request = b''
            while not request.endswith(payload):
                chunk = peer.recv(4096)
                if not chunk or len(request) + len(chunk) > 4096:
                    self.fail('remote upload did not arrive intact')
                request += chunk
            self.assertEqual(request, b'POST /upload HTTP/1.1\r\nContent-Length: 15\r\n\r\n' + payload)
            accepted = self.directory / 'accepted-upload'
            with accepted.open('wb') as output:
                output.write(payload)
                output.flush()
                os.fsync(output.fileno())
            manager.stop(unit, end)
            lifecycle.wait_quiescent(manager, owner, unit, invocation, end)
            state = lifecycle.state(manager, owner, unit, end)
            self.assertEqual(state['result'], 'signal')
            self.assertEqual(state['exec_status'], signal.SIGKILL)
            self.assertEqual(peer.recv(1), b'')
            self.assertEqual(accepted.read_bytes(), payload)
            manager.release(unit, end)
        finally:
            if reader is not None:
                os.close(reader)
            if writer is not None:
                os.close(writer)
            if peer is not None:
                peer.close()
            listener.close()
            if manager.dropin_name:
                subprocess.run(['/usr/bin/systemctl', 'stop', unit], check=False,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                manager.release(unit, deadline(10))
            manager.close()

    def test_root_owned_staging_rejects_preexisting_symlink(self):
        unit = "velnor-release-" + secrets.token_hex(16) + ".service"
        directory = Path("/run/systemd/system") / (unit + ".d")
        target = self.directory / "sentinel"
        target.mkdir()
        sentinel = target / "deadline.conf"
        sentinel.write_text("unchanged", encoding="ascii")
        directory.symlink_to(target, target_is_directory=True)
        self.addCleanup(directory.unlink, missing_ok=True)
        manager = bus.Manager(str(self.helper), 10, self.watch.authority)
        reader, writer = os.pipe()
        try:
            manager.subscribe(deadline(5))
            with self.assertRaises(bus.StartRejected) as rejected:
                manager.start(unit, reader, deadline(5))
            self.assertIsInstance(rejected.exception.__cause__, FileExistsError)
            self.assertTrue(directory.is_symlink())
            self.assertEqual(sentinel.read_text(encoding="ascii"), "unchanged")
        finally:
            os.close(reader)
            os.close(writer)
            manager.close()
            directory.unlink()
        manager = bus.Manager(str(self.helper), 10, self.watch.authority)
        reader, writer = os.pipe()
        try:
            end = deadline(10)
            manager.subscribe(end)
            manager.start(unit, reader, end)
            self.assertEqual(directory.stat().st_uid, 0)
            self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
            self.assertEqual((directory / "deadline.conf").stat().st_mode & 0o777, 0o600)
            timer = (directory / "deadline.conf").read_text(encoding="ascii")
            self.assertRegex(timer, r"^\[Unit\]\nJobTimeoutSec=[1-9][0-9]*s\n$")
            self.assertLessEqual(int(timer.split("=", 1)[1][:-2]), 10)
            os.close(writer)
            writer = None
            stop = manager.stop(unit, end)
            lifecycle.wait_job(manager, unit, stop, end)
            manager.release(unit, end)
            self.assertFalse(directory.exists())
        finally:
            os.close(reader)
            if writer is not None:
                os.close(writer)
            manager.close()
            if directory.exists():
                shutil.rmtree(directory)

    def test_malformed_kernel_cgroup_state_cannot_verify_cleanup(self):
        unit = "velnor-release-" + secrets.token_hex(16) + ".service"
        unit_dir = Path("/run/systemd/system") / (unit + ".d")
        manager = bus.Manager(str(self.helper), 10, self.watch.authority)
        reader, writer = os.pipe()
        try:
            end = deadline(10)
            owner = manager.subscribe(end)
            job = manager.start(unit, reader, end)
            lifecycle.wait_job(manager, unit, job, end)
            real_open = builtins.open
            attempts = []

            def vanished(path, *args, **kwargs):
                if str(path).endswith("/cgroup.events"):
                    attempts.append(str(path))
                    if len(attempts) == 1:
                        raise OSError(errno.ENODEV, "cgroup disappeared")
                return real_open(path, *args, **kwargs)

            with patch("builtins.open", side_effect=vanished):
                observed = lifecycle.state(manager, owner, unit, end)
            self.assertEqual(len(attempts), 2)
            self.assertTrue(observed["populated"])
            self.assertEqual(observed["control_group"], "/system.slice/" + unit)
            for code, expected in ((errno.ENODEV, 8), (errno.EIO, 1)):
                attempts.clear()

                def unavailable(path, *args, **kwargs):
                    attempts.append(str(path))
                    raise OSError(code, "cgroup unavailable")

                with self.subTest(code=code), patch("builtins.open", side_effect=unavailable):
                    with self.assertRaises(OSError):
                        lifecycle.state(manager, owner, unit, end)
                self.assertEqual(len(attempts), expected)
            for data in ("", "populated 0\npopulated 1\n", "populated 2\n"):
                with self.subTest(data=data), patch("builtins.open", return_value=io.StringIO(data)):
                    with self.assertRaisesRegex(bus.ManagerError, "invalid cgroup state"):
                        manager.state(unit, end)
            os.close(writer)
            writer = None
            stop = manager.stop(unit, end)
            lifecycle.wait_job(manager, unit, stop, end)
            actual = lifecycle.state(manager, owner, unit, end)
            self.assertFalse(actual["populated"])
            manager.release(unit, end)
        finally:
            os.close(reader)
            if writer is not None:
                os.close(writer)
            manager.close()
            subprocess.run(["/usr/bin/systemctl", "stop", unit], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=10)
            shutil.rmtree(unit_dir, ignore_errors=True)

    def test_real_manager_delivers_and_removes_owned_unit_dropin(self):
        digest = hashlib.sha256(b"dummy-credential").hexdigest()
        command = ("import hashlib,os,sys; value=sys.stdin.buffer.read(); "
                   "assert hashlib.sha256(value).hexdigest()==sys.argv[1]; "
                   "assert value not in b' '.join(x.encode() for x in sys.argv); "
                   "assert all(value not in x.encode() for x in os.environ.values())")
        body = json.dumps({"program": "/usr/bin/python3", "args": ["-I", "-S", "-c", command, digest],
                           "cwd": "/", "env": {}, "stdin": "dummy-credential"}).encode()
        manager = bus.Manager(str(self.helper), 10, self.watch.authority)
        try:
            end = deadline(45)
            frame = struct.pack(">QQI", end, os.stat("/proc/self/ns/time").st_ino, len(body)) + body
            lifecycle.supervise(manager, frame, end)
            self.assertIsNone(manager.dropin_name)
        finally:
            manager.close()

    def test_reobserved_cgroup_cannot_change_invocation(self):
        unit = "velnor-release-" + secrets.token_hex(16) + ".service"
        unit_dir = Path("/run/systemd/system") / (unit + ".d")
        manager = bus.Manager(str(self.helper), 10, self.watch.authority)
        readers = []
        writers = []
        real_open = builtins.open
        replaced = False
        end = deadline(20)
        try:
            manager.subscribe(end)
            reader, writer = os.pipe()
            readers.append(reader)
            writers.append(writer)
            lifecycle.wait_job(manager, unit, manager.start(unit, reader, end), end)

            def replace(path, *args, **kwargs):
                nonlocal replaced
                if str(path).endswith("/cgroup.events") and not replaced:
                    replaced = True
                    lifecycle.wait_job(manager, unit, manager.stop(unit, end), end)
                    manager.release(unit, end)
                    new_reader, new_writer = os.pipe()
                    readers.append(new_reader)
                    writers.append(new_writer)
                    lifecycle.wait_job(manager, unit, manager.start(unit, new_reader, end), end)
                    raise OSError(errno.ENODEV, "cgroup disappeared")
                return real_open(path, *args, **kwargs)

            with patch("builtins.open", side_effect=replace):
                with self.assertRaisesRegex(bus.ManagerError, "unit identity changed"):
                    manager.state(unit, end)
            self.assertTrue(replaced)
        finally:
            for descriptor in readers + writers:
                os.close(descriptor)
            subprocess.run(["/usr/bin/systemctl", "stop", unit], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=10)
            if manager.dropin_name:
                manager.release(unit, deadline(10))
            manager.close()
            shutil.rmtree(unit_dir, ignore_errors=True)

    def test_detached_term_resistant_descendant_is_gone_before_completion(self):
        ident = secrets.token_hex(16)
        unit = "velnor-release-" + ident + ".service"
        unit_dir = Path("/run/systemd/system") / (unit + ".d")
        channel = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        channel.bind(str(self.directory / "descendant"))
        os.chmod(self.directory / "descendant", 0o777)
        channel.listen(1)
        channel.settimeout(3)
        command = ("import os,signal,socket,sys,time\nr,w=os.pipe()\np=os.fork()\n"
                   "if p:\n os.close(w); os.read(r,1); os._exit(0)\n"
                   "os.close(r); os.setsid(); p=os.fork()\nif p: os._exit(0)\n"
                   "signal.signal(signal.SIGTERM,signal.SIG_IGN); s=socket.socket(socket.AF_UNIX); "
                   "s.connect(sys.argv[1]); s.send(str(os.getpid()).encode()); s.close(); "
                   "os.write(w,b'1'); time.sleep(300)")
        body = json.dumps({"program": "/usr/bin/python3", "args": ["-I", "-S", "-c", command,
                           str(self.directory / "descendant")], "cwd": "/", "env": {}, "stdin": ""}).encode()
        end = deadline(80)
        frame = struct.pack(">QQI", end, os.stat("/proc/self/ns/time").st_ino, len(body)) + body
        manager = bus.Manager(str(self.helper), 30, self.watch.authority)
        try:
            with patch.object(lifecycle.secrets, "token_hex", return_value=ident):
                try:
                    lifecycle.supervise(manager, frame, end)
                except lifecycle.SupervisorError as error:
                    self.assertEqual(str(error), "unit failed after verified cleanup")
            connection, _ = channel.accept()
            with connection:
                pid = connection.recv(32)
            self.assertTrue(pid.isdigit() and not pid.startswith(b"0"))
            self.assertFalse(Path("/proc", pid.decode("ascii")).exists())
            cgroup = Path("/sys/fs/cgroup/system.slice", unit, "cgroup.events")
            if cgroup.exists():
                self.assertIn("populated 0\n", cgroup.read_text(encoding="ascii"))
        finally:
            manager.close()
            channel.close()
            try:
                Path("/sys/fs/cgroup/system.slice", unit, "cgroup.kill").write_text("1\n", encoding="ascii")
            except FileNotFoundError:
                pass
            subprocess.run(["/usr/bin/systemctl", "stop", unit], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=25)
            shutil.rmtree(unit_dir, ignore_errors=True)

    def test_manager_runtime_reaps_delivered_child_after_supervisor_death(self):
        ident = secrets.token_hex(16)
        unit = "velnor-release-" + ident + ".service"
        source = str(root / "src")
        wrapper = (
            "import json,os,struct,sys,time; sys.path.insert(0,sys.argv[1]); "
            "import release_manager_bus as b,release_manager_lifecycle as l; "
            "sys.path.insert(0,os.path.join(sys.argv[1],'../tests')); from pathlib import Path; "
            "from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans; w=RootWatch(b,l,Path(sys.argv[1])); l.secrets.token_hex=lambda _:sys.argv[3]; "
            "m=b.Manager(sys.argv[2],6,w.authority); "
            "p=json.dumps({'program':'/usr/bin/python3','args':['-I','-S','-c','import time; time.sleep(300)'],"
            "'cwd':'/','env':{},'stdin':'dummy-credential'}).encode(); "
            "d=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+45_000_000_000; "
            "l.supervise(m,struct.pack('>QQI',d,os.stat('/proc/self/ns/time').st_ino,len(p))+p,d)"
        )
        parent = subprocess.Popen(["/usr/bin/python3", "-I", "-S", "-c", wrapper, source, str(self.helper), ident],
                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        unit_dir = Path("/run/systemd/system") / (unit + ".d")
        pid = None
        try:
            end = deadline(4)
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end and parent.poll() is None:
                result = subprocess.run(["/usr/bin/systemctl", "show", unit, "--property=ExecMainPID", "--value"],
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=2, check=False)
                value = result.stdout.strip()
                if value.isdigit() and value != b"0":
                    candidate = int(value)
                    try:
                        command = Path("/proc", str(candidate), "cmdline").read_bytes()
                    except FileNotFoundError:
                        command = b""
                    if b"time.sleep(300)" in command:
                        pid = candidate
                        break
                time.sleep(0.05)
            self.assertIsNotNone(pid, "helper did not deliver the frame before parent death")
            parent.kill()
            parent.wait(timeout=3)
            end = deadline(12)
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end and Path("/proc", str(pid)).exists():
                time.sleep(0.05)
            self.assertFalse(Path("/proc", str(pid)).exists())
            self.assertFalse(Path("/sys/fs/cgroup/system.slice", unit, "cgroup.procs").exists())
        finally:
            if parent.poll() is None:
                parent.kill()
                parent.wait(timeout=3)
            subprocess.run(["/usr/bin/systemctl", "stop", unit], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=10)
            shutil.rmtree(unit_dir, ignore_errors=True)

    def test_parent_death_while_accepted_start_is_queued(self):
        blocker = "velnor-test-blocker-" + secrets.token_hex(16) + ".service"
        unit = "velnor-release-" + secrets.token_hex(16) + ".service"
        unit_dir = Path("/run/systemd/system") / (unit + ".d")
        wrapper = (
            "import os,sys,time; sys.path.insert(0,sys.argv[1]); import release_manager_bus as b; "
            "import release_manager_lifecycle as l; from pathlib import Path; "
            "sys.path.insert(0,os.path.join(sys.argv[1],'../tests')); from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans; "
            "watch=RootWatch(b,l,Path(sys.argv[1])); print('watch-ready',flush=True); sys.stdin.readline(); m=b.Manager(sys.argv[2],2,watch.authority); d=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+10_000_000_000; "
            "m.subscribe(d); r,w=os.pipe(); job=m.start(sys.argv[3],r,d); os.close(r); "
            "print(job,flush=True); time.sleep(300)"
        )
        parent = None
        blocker_started = False
        try:
            parent = subprocess.Popen(["/usr/bin/python3", "-I", "-S", "-c", wrapper,
                                       str(root / "src"), str(self.helper), unit], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            self.assertTrue(select.select([parent.stdout], [], [], 10)[0], "watch was not established")
            self.assertEqual(parent.stdout.readline(), b"watch-ready\n")
            start_blocker(blocker, 20, unit)
            blocker_started = True
            parent.stdin.write(b"start\n")
            parent.stdin.flush()
            self.assertTrue(select.select([parent.stdout], [], [], 5)[0], "queued start was not acknowledged")
            self.assertTrue(parent.stdout.readline().startswith(b"/org/freedesktop/systemd1/job/"))
            jobs = subprocess.run(["/usr/bin/systemctl", "list-jobs", "--no-legend", "--plain"],
                                  capture_output=True, check=True, timeout=5).stdout
            self.assertIn(unit.encode(), jobs)
            parent.kill()
            parent.wait(timeout=3)
            end = deadline(8)
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                jobs = subprocess.run(["/usr/bin/systemctl", "list-jobs", "--no-legend", "--plain"],
                                      capture_output=True, check=True, timeout=5).stdout
                if unit.encode() not in jobs:
                    break
                time.sleep(0.05)
            self.assertNotIn(unit.encode(), jobs)
            subprocess.run(["/usr/bin/systemctl", "stop", blocker], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=True, timeout=10)
            self.assertFalse(Path("/sys/fs/cgroup/system.slice", unit, "cgroup.procs").exists())
        finally:
            if parent is not None:
                if parent.poll() is None:
                    parent.kill()
                    parent.wait(timeout=3)
                parent.stdout.close()
            subprocess.run(["/usr/bin/systemctl", "stop", unit, blocker], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=10)
            if blocker_started:
                finish_blocker(blocker)
            shutil.rmtree(unit_dir, ignore_errors=True)

    def test_real_queued_start_times_out_then_stop_converges(self):
        blocker = "velnor-test-blocker-" + secrets.token_hex(16) + ".service"
        unit = "velnor-release-" + secrets.token_hex(16) + ".service"
        manager = None
        reader = writer = None
        blocker_started = False
        try:
            start_blocker(blocker, 20, unit)
            blocker_started = True
            manager = bus.Manager(str(self.helper), 2, self.watch.authority)
            end = deadline(15)
            owner = manager.subscribe(end)
            reader, writer = os.pipe()
            job = manager.start(unit, reader, end)
            jobs = subprocess.run(["/usr/bin/systemctl", "list-jobs", "--no-legend", "--plain"],
                                  capture_output=True, check=True, timeout=5).stdout
            self.assertIn(unit.encode(), jobs, "candidate start was not queued")
            self.assertIn(blocker.encode(), jobs, "blocker start was not running")
            os.close(reader)
            reader = None
            while True:
                event = manager.event(end)
                self.assertIsNotNone(event)
                if event[0] == job:
                    break
            self.assertEqual(event, (job, unit, "timeout"))
            os.close(writer)
            writer = None
            stop = manager.stop(unit, end)
            lifecycle.wait_job(manager, unit, stop, end)
            state = lifecycle.state(manager, owner, unit, end)
            self.assertIn(state["active"], ("inactive", "failed"))
            self.assertIsNone(state["job"])
            self.assertFalse(state["populated"])
            manager.release(unit, end)
        finally:
            if reader is not None:
                os.close(reader)
            if writer is not None:
                os.close(writer)
            if manager is not None:
                manager.close()
            subprocess.run(["/usr/bin/systemctl", "stop", unit, blocker], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=10)
            if blocker_started:
                finish_blocker(blocker)


if __name__ == "__main__":
    unittest.main()
