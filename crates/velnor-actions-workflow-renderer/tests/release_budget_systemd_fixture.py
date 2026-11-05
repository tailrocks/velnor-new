# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import grp
import hashlib
import json
import os
import re
import selectors
import subprocess
import time

with open("/etc/os-release", encoding="utf-8") as info:
    release = info.read()
if 'ID=ubuntu' not in release.splitlines() or 'VERSION_ID="24.04"' not in release.splitlines():
    raise SystemExit("unexpected operating system image")
print("operating system: ubuntu-24.04", flush=True)

canary = "dummy-budget-canary"
expected = hashlib.sha256(canary.encode()).hexdigest()
service = r'''
import grp
import hashlib
import os
import subprocess
import sys
import time
frame = sys.stdin.readline().strip()
if hashlib.sha256(frame.encode()).hexdigest() != EXPECTED:
    raise SystemExit("dummy input unavailable")
if os.getuid() == 0:
    raise SystemExit("service must run unprivileged")
for name in ("sudo", "docker"):
    try:
        forbidden = grp.getgrnam(name).gr_gid
    except KeyError:
        continue
    if forbidden == os.getgid() or forbidden in os.getgroups():
        raise SystemExit("service has privileged group")
script = "import os,signal,time;os.setsid();pid=os.fork();" + \
    "os._exit(0) if pid else None;signal.signal(signal.SIGTERM,signal.SIG_IGN);" + \
    "print('uid=%d child=%d sid=%d' % (os.getuid(),os.getpid(),os.getsid(0)),flush=True);time.sleep(300)"
child = subprocess.Popen(["/usr/bin/python3", "-I", "-c", script])
child.wait()
time.sleep(0.5)
if MODE == "orphan":
    os._exit(0)
time.sleep(300)
'''.replace("EXPECTED", repr(expected))


def invoke(mode):
    unit = "velnor-budget-probe-" + str(os.getpid()) + "-" + mode
    command = [
        "/usr/bin/sudo", "-n", "/usr/bin/systemd-run", "--quiet", "--wait", "--pipe",
        "--unit=" + unit, "--property=DynamicUser=yes", "--property=NoNewPrivileges=yes",
        "--property=CapabilityBoundingSet=", "--property=AmbientCapabilities=", "--property=LimitCORE=0",
        "--property=Delegate=no", "--property=TimeoutStartSec=5s", "--property=RuntimeMaxSec=10s", "--property=TimeoutStopSec=2s",
        "--property=KillMode=control-group", "/usr/bin/python3", "-I", "-c", service.replace("MODE", repr(mode)),
    ]
    started = time.monotonic()
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        process.stdin.write(canary + "\n")
        process.stdin.flush()
        process.stdin.close()
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        if not selector.select(timeout=20):
            raise RuntimeError("transient service startup timed out")
        line = process.stdout.readline()
        match = re.fullmatch(r"uid=([1-9][0-9]*) child=([1-9][0-9]*) sid=([1-9][0-9]*)\n", line)
        if not match:
            raise RuntimeError("transient service did not establish escaped-session probe")
        pid = int(match.group(2))
        if pid == int(match.group(3)):
            raise RuntimeError("child was not double-forked into a separate session")
        control = subprocess.run(["/usr/bin/systemctl", "show", unit, "--value", "-p", "ControlGroup"], capture_output=True, text=True, timeout=5, check=False)
        cgroup = control.stdout.strip()
        main = subprocess.run(["/usr/bin/systemctl", "show", unit, "--value", "-p", "MainPID"], capture_output=True, text=True, timeout=5, check=False)
        if mode != "orphan" and (main.returncode != 0 or not main.stdout.strip().isdigit() or int(main.stdout.strip()) == int(match.group(3))):
            raise RuntimeError("child did not escape the service main session")
        if mode != "orphan" and (control.returncode != 0 or not cgroup.startswith("/")):
            raise RuntimeError("service cgroup unavailable")
        for command in (["/bin/ps", "-eo", "args"],
                        ["/usr/bin/systemctl", "show", unit, "-p", "Environment", "-p", "ExecStart"],
                        ["/usr/bin/sudo", "-n", "/usr/bin/journalctl", "--no-pager", "--unit", unit]):
            observed = subprocess.run(command, capture_output=True, text=True, timeout=5, check=False)
            if observed.returncode != 0 and mode != "orphan":
                raise RuntimeError("could not inspect service authority")
            if canary in observed.stdout or canary in observed.stderr:
                raise RuntimeError("dummy credential was exposed")
        if mode != "orphan":
            environment = subprocess.run(["/usr/bin/sudo", "-n", "/bin/cat", "/proc/" + str(pid) + "/environ"], capture_output=True, timeout=5, check=True)
            if canary.encode() in environment.stdout:
                raise RuntimeError("dummy credential leaked to child environment")
            status = subprocess.run(["/usr/bin/sudo", "-n", "/bin/cat", "/proc/" + str(pid) + "/status"], capture_output=True, text=True, timeout=5, check=True).stdout
            groups = re.search(r"^Groups:\s*([0-9 ]*)$", status, re.MULTILINE)
            if groups is None:
                raise RuntimeError("child groups unavailable")
            actual_groups = {int(value) for value in groups.group(1).split()}
            for name in ("sudo", "docker"):
                try:
                    forbidden = grp.getgrnam(name).gr_gid
                except KeyError:
                    continue
                if forbidden in actual_groups:
                    raise RuntimeError("child inherited a privileged group")
            if not re.search(r"^CapEff:\s+0+$", status, re.MULTILINE) or not re.search(r"^CapPrm:\s+0+$", status, re.MULTILINE) or not re.search(r"^NoNewPrivs:\s+1$", status, re.MULTILINE):
                raise RuntimeError("child retained capabilities")
        if mode == "cancel":
            subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/systemctl", "stop", unit], check=True, timeout=10, capture_output=True)
        process.wait(timeout=20)
        elapsed = time.monotonic() - started
        journal = subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/journalctl", "--no-pager", "--unit", unit], capture_output=True, text=True, timeout=5, check=True)
        if canary in journal.stdout or canary in journal.stderr:
            raise RuntimeError("dummy credential appeared in completed journal")
        if (mode == "orphan" and process.returncode != 0) or (mode != "orphan" and process.returncode == 0) or (mode == "timeout" and elapsed < 8):
            query = subprocess.run(["/usr/bin/systemctl", "show", unit, "-p", "Result", "-p", "ExecMainCode", "-p", "ExecMainStatus", "-p", "ActiveState", "-p", "LoadState"], capture_output=True, text=True, timeout=5, check=False)
            values = {}
            for line in query.stdout.splitlines():
                key, sep, value = line.partition("=")
                if sep and key in {"Result", "ExecMainCode", "ExecMainStatus", "ActiveState", "LoadState"} and re.fullmatch(r"[A-Za-z0-9_-]{1,40}", value):
                    values[key] = value
            events = "/sys/fs/cgroup" + cgroup + "/cgroup.events"
            populated = None
            if cgroup and os.path.exists(events):
                populated = "populated 1" in open(events, encoding="ascii").read()
            raise RuntimeError(f"service outcome mismatch: mode={mode} status={process.returncode} elapsed={elapsed:.1f} unit={values} pid_alive={os.path.exists('/proc/' + str(pid))} cgroup_populated={populated}")
        if not cgroup:
            state = subprocess.run(["/usr/bin/systemctl", "show", unit, "--value", "-p", "ActiveState"], capture_output=True, text=True, timeout=5, check=False)
            if state.returncode != 0 or state.stdout.strip() not in ("inactive", "failed"):
                raise RuntimeError("unit teardown was not confirmed")
        for _ in range(50):
            events = "/sys/fs/cgroup" + cgroup + "/cgroup.events"
            unpopulated = not cgroup or not os.path.exists(events) or "populated 0" in open(events, encoding="ascii").read()
            if not os.path.exists("/proc/" + str(pid)) and unpopulated:
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("escaped double-fork child survived")
    finally:
        subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/systemctl", "stop", unit], capture_output=True, timeout=10, check=False)
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/systemctl", "reset-failed", unit], capture_output=True, timeout=10, check=False)


invoke("timeout")
invoke("cancel")
invoke("orphan")
evidence = {"schema": 1, "source": "mise-task-v1", "head": os.environ["VELNOR_CHECK_HEAD"], "check_id": os.environ["VELNOR_CHECK_ID"], "platform": os.environ["VELNOR_CHECK_PLATFORM"], "scenarios": [{"id": name, "executed": True, "status": "passed"} for name in ("cancellation", "no-credential-exposure", "orphan", "timeout", "ubuntu-24.04")]}
with open(".velnor/hosted-budget-evidence.json", "x", encoding="utf-8") as output:
    json.dump(evidence, output, separators=(",", ":"))
print("hosted systemd timeout and cancellation removed escaped descendants")
