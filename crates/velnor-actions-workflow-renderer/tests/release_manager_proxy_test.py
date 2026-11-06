# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import array
import importlib.util
import os
from pathlib import Path
import secrets
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_manager_scenario_controller import finish_blocker, start_blocker
from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans
spec = importlib.util.spec_from_file_location("release_manager_bus", root / "src/release_manager_bus.py")
bus = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bus)
spec = importlib.util.spec_from_file_location("release_manager_lifecycle", root / "src/release_manager_lifecycle.py")
lifecycle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lifecycle)


class FaultProxy:
    def __init__(self, path):
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(path))
        self.listener.listen(1)
        self.paused = threading.Event()
        self.peers = []
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self):
        try:
            client, _ = self.listener.accept()
            upstream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            upstream.connect("/run/dbus/system_bus_socket")
            self.peers = [client, upstream]
            sender = threading.Thread(target=self.forward, args=(client, upstream, True), daemon=True)
            sender.start()
            self.forward(upstream, client, False)
        except OSError:
            pass
        finally:
            self.drop()

    def forward(self, source, target, from_client):
        previous = b""
        try:
            while True:
                data, ancillary, flags, _ = source.recvmsg(65536, socket.CMSG_SPACE(64 * array.array("i").itemsize))
                if not data or flags & (socket.MSG_CTRUNC | socket.MSG_TRUNC):
                    break
                if from_client:
                    if b"StartTransientUnit" in previous + data:
                        self.paused.set()
                    previous = (previous + data)[-32:]
                elif self.paused.is_set():
                    break
                sent = target.sendmsg([data], ancillary)
                for level, kind, payload in ancillary:
                    if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                        descriptors = array.array("i")
                        descriptors.frombytes(payload[:len(payload) - len(payload) % descriptors.itemsize])
                        for fd in descriptors:
                            os.close(fd)
                if sent < len(data):
                    target.sendall(data[sent:])
        except OSError:
            pass

    def drop(self):
        for peer in self.peers:
            try:
                peer.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                peer.close()
            except OSError:
                pass
        self.listener.close()


class RealReplyLossTest(unittest.TestCase):
    def setUp(self):
        existing = snapshot_roots()
        self.addCleanup(cleanup_orphans, existing)
        self.watch = RootWatch(bus, lifecycle, root / "src")
        self.addCleanup(self.watch.close)

    def test_production_constructor_rejects_inherited_fake_broker(self):
        with tempfile.TemporaryDirectory(prefix="velnor-fake-broker-", dir="/run") as directory:
            path = Path(directory) / "socket"
            fake = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            fake.bind(str(path))
            fake.listen(1)
            try:
                program = ("import sys;sys.path.insert(0,sys.argv[1]);import release_manager_bus as b; "
                           "import os,pathlib;sys.path.insert(0,str(pathlib.Path(sys.argv[1]).parent/'tests')); "
                           "from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans;import release_manager_lifecycle as l; "
                           "address=os.environ.pop('DBUS_SYSTEM_BUS_ADDRESS');watch=RootWatch(b,l,pathlib.Path(sys.argv[1])); "
                           "os.environ['DBUS_SYSTEM_BUS_ADDRESS']=address; "
                           "\ntry: b.Manager('/usr/bin/true',10,watch.authority)"
                           "\nfinally: watch.close()")
                result = subprocess.run(["/usr/bin/python3", "-I", "-S", "-c", program, str(root / "src")],
                                        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        env={"DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(path),
                                             "PATH": "/usr/bin:/bin"}, timeout=5, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(select.select([fake], [], [], 0)[0], "production manager connected to fake broker")
            finally:
                fake.close()

    def test_accepted_start_runs_late_after_reply_loss_without_input(self):
        if os.geteuid() != 0:
            raise RuntimeError("systemd fixture requires root")
        scratch = Path(tempfile.mkdtemp(prefix="velnor-proxy-", dir="/run"))
        self.addCleanup(shutil.rmtree, scratch, ignore_errors=True)
        scratch.chmod(0o755)
        shared = Path("/run/systemd/system/velnor-release-.service.d")
        shared.mkdir(mode=0o700)
        def remove_shared():
            if shared.exists():
                (shared / "wait.conf").unlink(missing_ok=True)
                shared.rmdir()
        self.addCleanup(remove_shared)
        blocker = "velnor-test-blocker-" + secrets.token_hex(16) + ".service"
        ident = secrets.token_hex(16)
        unit = "velnor-release-" + ident + ".service"
        unit_dir = Path("/run/systemd/system") / (unit + ".d")
        helper = scratch / "helper.py"
        shutil.copyfile(root / "src/release_supervisor_exec.py", helper)
        helper.chmod(0o555)
        sentinel = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(sentinel.close)
        sentinel.bind(str(scratch / "sentinel"))
        os.chmod(scratch / "sentinel", 0o777)
        sentinel.listen(1)
        proxy = FaultProxy(scratch / "proxy")
        self.addCleanup(proxy.drop)
        observer = bus.Manager(str(helper), 15, self.watch.authority)
        parent = None
        blocker_started = False
        try:
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 15_000_000_000
            observer.subscribe(end)
            wrapper = (
                "import json,os,struct,sys,time; sys.path.insert(0,sys.argv[1]); "
                "import release_manager_bus as b,release_manager_lifecycle as l; "
                "from pathlib import Path;sys.path.insert(0,str(Path(sys.argv[1]).parent/'tests')); "
                "from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans;address=os.environ.pop('DBUS_SYSTEM_BUS_ADDRESS'); "
                "watch=RootWatch(b,l,Path(sys.argv[1]));print('watch-ready',flush=True);sys.stdin.readline();os.environ['DBUS_SYSTEM_BUS_ADDRESS']=address; "
                "l.secrets.token_hex=lambda _:sys.argv[3]; "
                "m=b.Manager(sys.argv[2],15,watch.authority,transport=lambda x:x._check(x.library.sd_bus_open_system(__import__('ctypes').byref(x.bus))),root_bootstrap=True); "
                "command=\"import socket,sys;s=socket.socket(socket.AF_UNIX);s.connect(sys.argv[1]);s.send(b'x')\"; "
                "p=json.dumps({'program':'/usr/bin/python3','args':['-I','-S','-c',command,sys.argv[4]],"
                "'cwd':'/','env':{},'stdin':'dummy-credential'}).encode(); "
                "d=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+45_000_000_000; "
                "\ntry: l.supervise(m,struct.pack('>QQI',d,os.stat('/proc/self/ns/time').st_ino,len(p))+p,d)"
                "\nexcept Exception as e: print(str(e),flush=True)"
                "\nfinally: m.close(); time.sleep(15); watch.close()"
            )
            environment = {"DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(scratch / "proxy"), "PATH": "/usr/bin:/bin"}
            parent = subprocess.Popen(["/usr/bin/python3", "-I", "-S", "-c", wrapper, str(root / "src"),
                                       str(helper), ident, str(scratch / "sentinel")], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment)
            self.assertTrue(select.select([parent.stdout], [], [], 10)[0], "watch was not established")
            self.assertEqual(parent.stdout.readline(), b"watch-ready\n")
            (shared / "wait.conf").write_text("[Unit]\nAfter=" + blocker + "\n", encoding="ascii")
            start_blocker(blocker, 4)
            blocker_started = True
            parent.stdin.write(b"start\n")
            parent.stdin.flush()
            self.assertTrue(proxy.paused.wait(5), "StartTransientUnit never reached the real bus")
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 3_000_000_000
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                jobs = subprocess.run(["/usr/bin/systemctl", "list-jobs", "--no-legend", "--plain"],
                                      capture_output=True, check=True, timeout=5).stdout
                if unit.encode() in jobs:
                    break
                time.sleep(0.02)
            self.assertIn(unit.encode(), jobs, "manager did not accept the queued start")
            proxy.drop()
            self.assertTrue(select.select([parent.stdout], [], [], 5)[0], "caller did not return")
            self.assertEqual(parent.stdout.readline().strip(), b"unit cleanup uncertain")
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 10_000_000_000
            while True:
                event = observer.event(end)
                self.assertIsNotNone(event, "late start did not finish")
                if event[1] == unit:
                    break
            self.assertEqual(event[2], "done")
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 3_000_000_000
            cgroup = Path("/sys/fs/cgroup/system.slice", unit, "cgroup.procs")
            while cgroup.exists() and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                time.sleep(0.05)
            self.assertFalse(cgroup.exists(), "late helper did not receive EOF")
            self.assertIsNone(parent.poll(), "caller did not remain alive during late-start verification")
            self.assertFalse(select.select([sentinel], [], [], 1)[0], "late helper executed framed payload")
        finally:
            proxy.drop()
            observer.close()
            sentinel.close()
            if parent is not None and parent.poll() is None:
                parent.kill()
                parent.wait(timeout=3)
            if parent is not None:
                parent.stdout.close()
                parent.stderr.close()
            subprocess.run(["/usr/bin/systemctl", "stop", unit, blocker], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=False, timeout=10)
            if blocker_started:
                finish_blocker(blocker)
            shutil.rmtree(unit_dir, ignore_errors=True)
            (shared / "wait.conf").unlink(missing_ok=True)
            shared.rmdir()
            shutil.rmtree(scratch)


if __name__ == "__main__":
    unittest.main()
