# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import unittest


source = Path(__file__).resolve().with_name("release_manager_scenario_controller.py")
spec = importlib.util.spec_from_file_location("scenario_controller", source)
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


class ScenarioControllerTest(unittest.TestCase):
    def run_fixture(self, root, script, start=True, synthetic_ledger=True):
        tests = root / "tests"
        source = root / "src"
        tests.mkdir()
        source.mkdir()
        original = Path(__file__).resolve().parents[1] / "src"
        for name in ("release_manager_bus.py", "release_manager_lifecycle.py", "release_supervisor_exec.py",
                     "release_bootstrap_watch.py", "release_watch_admission.py", "release_launcher_watch.py", "release_supervisor_claim.py"):
            shutil.copyfile(original / name, source / name)
        shutil.copyfile(Path(__file__).resolve().with_name("release_manager_scenario_controller.py"),
                        tests / "release_manager_scenario_controller.py")
        shutil.copyfile(Path(__file__).with_name("release_watch_fixture.py"), tests / "release_watch_fixture.py")
        ledger = (("import os\nwith open(os.environ['VELNOR_SCENARIO_LEDGER'],'a') as f:\n"
                   " f.write('0'*32+chr(10)+'!00000001'+chr(10))\n") if synthetic_ledger else "")
        (tests / "release_manager_qualification.py").write_text(ledger + script)
        spec = importlib.util.spec_from_file_location("scenario_fixture", tests / "release_manager_scenario_controller.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        if start:
            module.run_fixed()
        return tests

    def test_live_scenario_unit_cannot_produce_success(self):
        units = []

        def scenario():
            unit = "velnor-release-" + secrets.token_hex(16) + ".service"
            units.append(unit)
            subprocess.run(["/usr/bin/systemd-run", "--unit=" + unit, "--property=RuntimeMaxSec=30s",
                            "/usr/bin/sleep", "20"], check=True, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=5)

        try:
            with self.assertRaises(controller.ScenarioError):
                controller.run(scenario)
        finally:
            if units:
                subprocess.run(["/usr/bin/systemctl", "stop", *units], stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=10, check=False)

    def test_allocated_name_is_reserved_once(self):
        inventory = controller.UnitInventory()
        value = inventory.allocate(16)
        inventory.generate = lambda _: value
        with self.assertRaisesRegex(controller.ScenarioError, "reused"):
            inventory.allocate(16)

    def test_existing_shared_prefix_staging_is_rejected(self):
        path = Path("/run/systemd/system/velnor-release-.service.d")
        path.symlink_to("/nonexistent/velnor-scenario-dangling")
        try:
            with tempfile.TemporaryDirectory(prefix="velnor-scenario-prefix-", dir="/run") as directory:
                with self.assertRaises(RuntimeError):
                    self.run_fixture(Path(directory), "pass")
        finally:
            path.unlink()

    def test_existing_unit_dropin_symlink_is_rejected(self):
        ident = os.urandom(16).hex()
        unit = "velnor-release-" + ident + ".service"
        path = Path("/run/systemd/system") / (unit + ".d")
        path.symlink_to("/nonexistent/velnor-scenario-dangling")
        try:
            with self.assertRaises(controller.ScenarioError):
                controller.UnitInventory().check(ident, unused=True)
        finally:
            path.unlink()

    def test_unregistered_live_unit_cannot_produce_success(self):
        units = []

        def scenario():
            unit = "velnor-release-" + os.urandom(16).hex() + ".service"
            units.append(unit)
            subprocess.run(["/usr/bin/systemd-run", "--unit=" + unit, "--property=RuntimeMaxSec=30s",
                            "/usr/bin/sleep", "20"], check=True, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=5)

        try:
            with self.assertRaises(controller.ScenarioError):
                controller.run(scenario)
        finally:
            if units:
                subprocess.run(["/usr/bin/systemctl", "stop", *units], stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=10, check=False)

    def test_leaked_scenario_dropin_cannot_produce_success(self):
        directories = []

        def scenario():
            unit = "velnor-release-" + secrets.token_hex(16) + ".service"
            directory = Path("/run/systemd/system") / (unit + ".d")
            directory.mkdir(mode=0o700)
            directories.append(directory)
            (directory / "deadline.conf").write_text("[Unit]\nJobTimeoutSec=30s\n")

        try:
            with self.assertRaises(controller.ScenarioError):
                controller.run(scenario)
        finally:
            for directory in directories:
                (directory / "deadline.conf").unlink(missing_ok=True)
                directory.rmdir()

    def test_queued_start_and_bounded_blocker_cannot_produce_success(self):
        inventory = controller.UnitInventory()
        blocker = "velnor-test-blocker-" + inventory.allocate(16) + ".service"
        ident = inventory.allocate(16)
        unit = "velnor-release-" + ident + ".service"
        directory = Path("/run/systemd/system") / (unit + ".d")
        directory.mkdir(mode=0o700)
        started = False
        try:
            (directory / "after.conf").write_text("[Unit]\nAfter=" + blocker + "\nJobTimeoutSec=30s\n")
            controller.start_blocker(blocker, 20)
            started = True
            properties = controller.command(["show", blocker, "--property=JobTimeoutUSec",
                                             "--property=TimeoutStartUSec", "--property=TimeoutStopUSec"])
            self.assertIn("JobTimeoutUSec=30s\n", properties)
            self.assertIn("TimeoutStartUSec=30s\n", properties)
            self.assertIn("TimeoutStopUSec=5s\n", properties)
            subprocess.run(["/usr/bin/systemd-run", "--no-block", "--unit=" + unit,
                            "--property=Type=oneshot", "/usr/bin/true"], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
            end = time.monotonic() + 5
            while time.monotonic() < end:
                if unit in controller.command(["list-jobs", "--no-legend", "--plain", "--full", "--no-pager"]):
                    break
                time.sleep(0.02)
            with self.assertRaises(controller.ScenarioError):
                inventory.check(ident, allow_dropin=True)
        finally:
            subprocess.run(["/usr/bin/systemctl", "stop", unit, blocker], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=10, check=False)
            if started:
                controller.finish_blocker(blocker)
            (directory / "after.conf").unlink(missing_ok=True)
            directory.rmdir()

    def test_qualification_capability_does_not_reach_publisher_service(self):
        with tempfile.TemporaryDirectory(prefix="velnor-capabilities-", dir="/run") as directory:
            root = Path(directory)
            candidate_source = Path(__file__).resolve().parents[1] / "src"
            source = root / "src"
            helper = root / "helper.py"
            helper.write_bytes((candidate_source / "release_supervisor_exec.py").read_bytes())
            helper.chmod(0o555)
            outer = root / "outer.json"
            inner = root / "inner.json"
            child = ("import json,pathlib;v=pathlib.Path('/proc/self/status').read_text().splitlines();"
                     "pathlib.Path(" + repr(str(inner)) + ").write_text(json.dumps(v))")
            script = ("import json,os,pathlib,struct,sys,time;sys.path.insert(0," + repr(str(source)) + ");"
                      "import release_manager_bus as b,release_manager_lifecycle as l;"
                      "pathlib.Path(" + repr(str(outer)) + ").write_text(json.dumps(pathlib.Path('/proc/self/status').read_text().splitlines()));"
                      "p=json.dumps({'program':'/usr/bin/python3','args':['-I','-S','-c'," + repr(child) + "],"
                      "'cwd':'/','env':{},'stdin':''}).encode();"
                      "d=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+45_000_000_000;"
                      "sys.path.insert(0," + repr(str(root / "tests")) + ");from release_watch_fixture import RootWatch;"
                      "watch=RootWatch(b,l,pathlib.Path(" + repr(str(source)) + "));"
                      "m=b.Manager(" + repr(str(helper)) + ",15,watch.authority,root_bootstrap=True);"
                      "l.supervise(m,struct.pack('>QQI',d,os.stat('/proc/self/ns/time').st_ino,len(p))+p,d);m.close();watch.close()")
            failure = root / "child-error"
            script = ("import pathlib,traceback\ntry:\n exec(" + repr(script) + ")\n"
                      "except Exception:\n pathlib.Path(" + repr(str(failure))
                      + ").write_text(traceback.format_exc()[:8192]); raise\n")
            try:
                self.run_fixture(root, script)
            except Exception as error:
                detail = failure.read_text() if failure.is_file() else "child diagnostic unavailable"
                raise AssertionError("nested capability test failed: " + detail) from error
            def capabilities(path):
                return {line.split(":", 1)[0]: line.split(":", 1)[1].strip()
                        for line in json.loads(path.read_text()) if ":" in line}
            qualified = capabilities(outer)
            publisher = capabilities(inner)
            self.assertEqual(int(qualified["CapBnd"], 16), 1 << 19)
            self.assertEqual(int(qualified["CapEff"], 16), 1 << 19)
            self.assertEqual(qualified["Uid"].split(), ["0", "0", "0", "0"])
            self.assertEqual(qualified["NoNewPrivs"], "1")
            self.assertEqual(int(publisher["CapBnd"], 16), 0)
            self.assertEqual(int(publisher["CapEff"], 16), 0)
            self.assertEqual(publisher["NoNewPrivs"], "1")

    def test_detached_child_cannot_start_unit_after_success(self):
        with tempfile.TemporaryDirectory(prefix="velnor-scenario-late-", dir="/run") as directory:
            root = Path(directory)
            unit = "velnor-release-" + os.urandom(16).hex() + ".service"
            marker = root / "started"
            script = ("import os,signal,subprocess,time;r,w=os.pipe();p=os.fork();\n"
                      "if p:\n os.close(w);os.read(r,1);os._exit(0)\n"
                      "os.close(r);os.setsid();signal.signal(signal.SIGTERM,signal.SIG_IGN);"
                      "os.write(w,b'x');time.sleep(2);"
                      "subprocess.run(['/usr/bin/systemd-run','--unit=" + unit + "',"
                      "'--property=RuntimeMaxSec=45s','/usr/bin/sleep','40'],check=True);"
                      "open(" + repr(str(marker)) + ",'w').write('started');time.sleep(300)\n")
            try:
                with self.assertRaises(Exception):
                    self.run_fixture(root, script)
                self.assertEqual(marker.read_text(), "started")
                active = subprocess.run(["/usr/bin/systemctl", "is-active", unit], capture_output=True,
                                        timeout=5, check=False)
                self.assertEqual(active.stdout.strip(), b"active")
            finally:
                subprocess.run(["/usr/bin/systemctl", "stop", unit], stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=10, check=False)

    def test_controller_death_has_no_receipt_and_unit_expires(self):
        with tempfile.TemporaryDirectory(prefix="velnor-scenario-death-", dir="/run") as directory:
            root = Path(directory)
            channel = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            channel.bind(str(root / "ready"))
            channel.listen(1)
            channel.settimeout(10)
            script = ("import os,socket,time;s=socket.socket(socket.AF_UNIX);"
                      "s.connect(" + repr(str(root / "ready")) + ");s.send(str(os.getpid()).encode());"
                      "s.close();time.sleep(300)")
            tests = self.run_fixture(root, script, start=False)
            marker = root / "passed"
            wrapper = ("import importlib.util,pathlib;"
                       "s=importlib.util.spec_from_file_location('c'," + repr(str(tests / 'release_manager_scenario_controller.py')) + ");"
                       "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);"
                       "m.RUNTIME_SECONDS=2;m.DEADLINE_SECONDS=45;m.run_fixed();"
                       "pathlib.Path(" + repr(str(marker)) + ").write_text('passed')")
            before = set(Path("/run").glob("velnor-scenario-controller-*"))
            parent = subprocess.Popen(["/usr/bin/python3", "-I", "-S", "-c", wrapper],
                                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            unit = None
            try:
                connection, _ = channel.accept()
                with connection:
                    pid = int(connection.recv(32))
                groups = Path("/proc", str(pid), "cgroup").read_text().splitlines()
                unit = groups[0].rsplit("/", 1)[-1]
                self.assertRegex(unit, r"^velnor-release-[0-9a-f]{32}\.service$")
                parent.kill()
                parent.wait(timeout=5)
                end = time.monotonic() + 15
                while Path("/proc", str(pid)).exists() and time.monotonic() < end:
                    time.sleep(0.05)
                self.assertFalse(Path("/proc", str(pid)).exists())
                self.assertFalse(marker.exists())
                ident = unit.removeprefix("velnor-release-").removesuffix(".service")
                controller.UnitInventory().check(ident, allow_dropin=True)
            finally:
                channel.close()
                if parent.poll() is None:
                    parent.kill()
                    parent.wait(timeout=5)
                if unit:
                    subprocess.run(["/usr/bin/systemctl", "stop", unit], stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, timeout=10, check=False)
                    ident = unit.removeprefix("velnor-release-").removesuffix(".service")
                    controller.UnitInventory().check(ident, allow_dropin=True)
                    dropin = Path("/run/systemd/system") / (unit + ".d")
                    (dropin / "deadline.conf").unlink(missing_ok=True)
                    if dropin.exists():
                        dropin.rmdir()
                for created in set(Path("/run").glob("velnor-scenario-controller-*")) - before:
                    shutil.rmtree(created)

    def test_real_child_records_two_unit_identities(self):
        with tempfile.TemporaryDirectory(prefix="velnor-scenario-ledger-", dir="/run") as directory:
            root = Path(directory)
            script = ("import importlib.util,os,pathlib,secrets,subprocess;"
                      "p=pathlib.Path(__file__).with_name('release_manager_scenario_controller.py');"
                      "s=importlib.util.spec_from_file_location('c',p);"
                      "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);\n"
                      "def suite():\n"
                      " for _ in range(2):\n"
                      "  unit='velnor-release-'+secrets.token_hex(16)+'.service'\n"
                      "  subprocess.run(['/usr/bin/systemd-run','--wait','--unit='+unit,'/usr/bin/true'],"
                      "check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)\n"
                      "m.run(suite,os.environ['VELNOR_SCENARIO_LEDGER'])\n")
            self.run_fixture(root, script, synthetic_ledger=False)

    def test_malformed_or_incomplete_identity_ledger_is_rejected(self):
        ident = "0" * 32
        cases = ("", ident + "\n", ident + "\n!00000002\n", ident + "\n" + ident + "\n!00000002\n",
                 ident + "\n!00000001\n!00000001\n", ident + "\n!00000001")
        for data in cases:
            with self.subTest(data=data), tempfile.TemporaryDirectory(prefix="velnor-scenario-bad-ledger-", dir="/run") as directory:
                script = ("import os\nwith open(os.environ['VELNOR_SCENARIO_LEDGER'],'w') as f:\n"
                          " f.write(" + repr(data) + ")\n")
                with self.assertRaises(RuntimeError):
                    self.run_fixture(Path(directory), script, synthetic_ledger=False)


if __name__ == "__main__":
    required = {
        "test_live_scenario_unit_cannot_produce_success",
        "test_allocated_name_is_reserved_once",
        "test_existing_shared_prefix_staging_is_rejected",
        "test_existing_unit_dropin_symlink_is_rejected",
        "test_unregistered_live_unit_cannot_produce_success",
        "test_leaked_scenario_dropin_cannot_produce_success",
        "test_queued_start_and_bounded_blocker_cannot_produce_success",
        "test_qualification_capability_does_not_reach_publisher_service",
        "test_detached_child_cannot_start_unit_after_success",
        "test_controller_death_has_no_receipt_and_unit_expires",
        "test_real_child_records_two_unit_identities",
        "test_malformed_or_incomplete_identity_ledger_is_rejected",
    }
    if set(unittest.defaultTestLoader.getTestCaseNames(ScenarioControllerTest)) != required:
        raise SystemExit("controller scenario inventory mismatch")
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ScenarioControllerTest)
    result = unittest.TextTestRunner(verbosity=1).run(suite)
    if not result.wasSuccessful() or result.testsRun != len(required) or result.skipped or result.expectedFailures:
        raise SystemExit("controller qualification failed")
