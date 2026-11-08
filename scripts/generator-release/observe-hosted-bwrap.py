#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import re
import secrets
import select
import signal
import subprocess
import tempfile
import time

BWRAP = '/usr/bin/bwrap'
MAX_PROC_ENTRIES = 131072
MAX_CMDLINE_BYTES = 16384
CHILD = r'''
import json, os, select, socket, sys, time
read_fd, write_fd = os.pipe()
child = os.fork()
if child == 0:
    os.close(read_fd)
    os.setsid()
    os.write(write_fd, str(os.getpid()).encode('ascii') + b'\n')
    os.close(write_fd)
    time.sleep(20)
    os._exit(0)
os.close(write_fd)
if not select.select([read_fd], [], [], 3)[0]:
    os._exit(1)
handshake = os.read(read_fd, 16)
if not handshake.endswith(b'\n') or not handshake[:-1].isdigit():
    os._exit(1)
setsid_pid = int(handshake)
os.close(read_fd)
try:
    with open('/tmp/probe/write-check', 'w'):
        pass
    ro = False
except OSError:
    ro = True
sock = socket.socket(socket.AF_UNIX)
try:
    sock.connect('/run/docker.sock')
    docker_absent = False
except OSError:
    docker_absent = True
finally:
    sock.close()
sock = socket.socket(socket.AF_INET)
sock.settimeout(0.5)
try:
    sock.connect(('1.1.1.1', 53))
    network_unreachable = False
except OSError:
    network_unreachable = True
finally:
    sock.close()
try:
    socket.getaddrinfo(sys.argv[1] + '.example.com', 80)
    dns_resolution_failed = False
except socket.gaierror:
    dns_resolution_failed = True
print(json.dumps({'readonly_write_rejected': ro,
                  'setsid_namespace_pid': setsid_pid,
                  'docker_socket_unreachable': docker_absent,
                  'resolver_config_absent': not os.path.exists('/etc/resolv.conf'),
                  'network_connection_failed': network_unreachable,
                  'dns_resolution_failed': dns_resolution_failed,
                  'loopback_only': all(name == 'lo' for _, name in socket.if_nameindex())}), flush=True)
time.sleep(20)
'''


def proc_entries():
    entries = []
    with os.scandir('/proc') as scan:
        for entry in scan:
            entries.append(entry.name)
            if len(entries) > MAX_PROC_ENTRIES:
                raise ValueError('proc entry limit')
    return entries


def matching_pids(token):
    matches = []
    for name in proc_entries():
        if not name.isdecimal():
            continue
        base = pathlib.Path('/proc') / name
        try:
            if base.stat().st_uid != os.geteuid():
                continue
            with (base / 'cmdline').open('rb') as stream:
                cmdline = stream.read(MAX_CMDLINE_BYTES + 1)
            if len(cmdline) > MAX_CMDLINE_BYTES:
                raise ValueError('proc command line limit')
            args = cmdline.split(b'\0')
            if token.encode('ascii') in args:
                matches.append(int(name))
        except (FileNotFoundError, ProcessLookupError):
            continue
    return matches


def owned_namespaces():
    result = set()
    for name in proc_entries():
        if not name.isdecimal():
            continue
        base = pathlib.Path('/proc') / name
        try:
            if base.stat().st_uid == os.geteuid():
                result.add(os.readlink(base / 'ns/pid'))
        except (FileNotFoundError, ProcessLookupError):
            continue
    return result


def session_leaders(pids):
    leaders = []
    for pid in pids:
        try:
            with open(f'/proc/{pid}/stat', 'rb') as stream:
                data = stream.read(4097)
            if len(data) > 4096 or b') ' not in data:
                raise ValueError('invalid proc stat')
            fields = data.rsplit(b') ', 1)[1].split()
            if len(fields) < 4:
                raise ValueError('short proc stat')
            if int(fields[3]) == pid:
                leaders.append(pid)
        except (FileNotFoundError, ProcessLookupError):
            continue
    return leaders


def namespace_pid(pid):
    with open(f'/proc/{pid}/status', 'rb') as stream:
        data = stream.read(65537)
    if len(data) > 65536:
        raise ValueError('proc status limit')
    records = [line.split() for line in data.splitlines() if line.startswith(b'NSpid:')]
    if len(records) != 1 or len(records[0]) < 2 or not all(value.isdigit() for value in records[0][1:]):
        raise ValueError('invalid NSpid')
    return int(records[0][-1])


def read_ready(stream, timeout):
    fd = stream.fileno()
    os.set_blocking(fd, False)
    deadline = time.monotonic() + timeout
    data = bytearray()
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
            raise TimeoutError('readiness timeout')
        try:
            block = os.read(fd, 4097 - len(data))
        except BlockingIOError:
            continue
        if not block:
            raise ValueError('readiness ended before newline')
        data.extend(block)
        if len(data) > 4096:
            raise ValueError('readiness limit')
        if b'\n' in data:
            if not data.endswith(b'\n') or data.count(b'\n') != 1:
                raise ValueError('invalid readiness framing')
            return bytes(data)


def parse_ready(line):
    details = json.loads(line)
    keys = {'readonly_write_rejected', 'docker_socket_unreachable', 'resolver_config_absent',
            'network_connection_failed', 'dns_resolution_failed', 'loopback_only', 'setsid_namespace_pid'}
    if (not isinstance(details, dict) or set(details) != keys
            or type(details['setsid_namespace_pid']) is not int or details['setsid_namespace_pid'] <= 1
            or not all(type(details[key]) is bool for key in keys - {'setsid_namespace_pid'})):
        raise ValueError('invalid readiness')
    return details


def observe():
    result = {'observation_only': True, 'completed': False, 'cleanup_guaranteed': False,
              'process_scan_scope': 'same-uid-only',
              'network_failure_interpretation': 'inconclusive-without-external-positive-control',
              'readonly_failure_interpretation': 'oserror-not-proof-of-mount',
              'socket_failure_interpretation': 'unreachable-not-proof-of-absence'}
    if not os.path.isfile(BWRAP) or not pathlib.Path('/proc').is_dir():
        result['reason'] = 'unavailable'
        return result
    version = subprocess.run([BWRAP, '--version'], stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             timeout=2, check=False, env={'PATH': '/usr/bin:/bin'})
    version_text = version.stdout.decode('ascii', errors='replace').strip()
    if version.returncode != 0 or not re.fullmatch(r'bubblewrap [0-9.]+', version_text):
        result['reason'] = 'invalid_version'
        return result
    result['bubblewrap_version'] = version_text
    token = 'velnor-bwrap-' + secrets.token_hex(16)
    sentinel = subprocess.Popen(['/usr/bin/sleep', '25'], stdin=subprocess.DEVNULL,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    child = None
    try:
        with tempfile.TemporaryDirectory(prefix='velnor-bwrap-observation-', dir=os.environ['RUNNER_TEMP']) as root:
            cmd = [BWRAP, '--unshare-pid', '--unshare-net', '--die-with-parent', '--new-session',
                   '--ro-bind', '/', '/', '--tmpfs', '/tmp', '--tmpfs', '/run', '--tmpfs', '/etc',
                   '--ro-bind', root, '/tmp/probe', '--proc', '/proc', '--dev', '/dev', '--',
                   '/usr/bin/python3', '-I', '-S', '-c', CHILD, token]
            child = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, env={'PATH': '/usr/bin:/bin'})
            try:
                line = read_ready(child.stdout, 8)
            except TimeoutError:
                result['reason'] = 'readiness_timeout'
                return result
            except ValueError:
                result['reason'] = 'invalid_readiness'
                return result
            try:
                details = parse_ready(line)
            except (ValueError, UnicodeDecodeError):
                result['reason'] = 'invalid_readiness'
                return result
            observed = [pid for pid in matching_pids(token) if pid != child.pid]
            if len(observed) < 2:
                result['reason'] = 'descendants_not_observed'
                return result
            own_namespace = os.readlink('/proc/self/ns/pid')
            namespaces = {os.readlink(f'/proc/{pid}/ns/pid') for pid in observed}
            namespaces.discard(own_namespace)
            if len(namespaces) != 1:
                result['reason'] = 'namespace_not_identified'
                return result
            namespace = namespaces.pop()
            leaders = [pid for pid in session_leaders(observed)
                       if os.readlink(f'/proc/{pid}/ns/pid') == namespace
                       and namespace_pid(pid) == details['setsid_namespace_pid']]
            if len(leaders) != 1:
                result['reason'] = 'setsid_descendant_not_identified'
                return result
            result['setsid_descendant_identified'] = True
            os.kill(child.pid, signal.SIGKILL)
            child.wait(timeout=3)
            deadline = time.monotonic() + 4
            while matching_pids(token) and time.monotonic() < deadline:
                time.sleep(0.05)
            result.update(details)
            result['descendants_disappeared'] = not matching_pids(token)
            result['observed_namespace_disappeared'] = namespace not in owned_namespaces()
            result['unrelated_sentinel_survived'] = sentinel.poll() is None
            result['completed'] = True
            return result
    finally:
        if child is not None and child.poll() is None:
            child.kill()
            child.wait(timeout=3)
        if sentinel.poll() is None:
            sentinel.terminate()
        sentinel.wait(timeout=3)


if __name__ == '__main__':
    try:
        print(json.dumps(observe(), sort_keys=True, separators=(',', ':')))
    except (KeyError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(json.dumps({'observation_only': True, 'completed': False,
                          'cleanup_guaranteed': False, 'reason': type(error).__name__},
                         sort_keys=True, separators=(',', ':')))
