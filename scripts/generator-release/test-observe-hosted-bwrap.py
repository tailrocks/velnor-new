#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import io
import json
import os
import pathlib
import subprocess
import tempfile
import types
from unittest import mock

SCRIPT = pathlib.Path(__file__).with_name('observe-hosted-bwrap.py')
spec = importlib.util.spec_from_file_location('bwrap_observer', SCRIPT)
observer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observer)


def main():
    stat_data = {
        '/proc/111/stat': b'111 (python) S 1 111 1 0\n',
        '/proc/222/stat': b'222 (python (fork)) S 111 222 222 0\n',
    }
    with mock.patch('builtins.open', side_effect=lambda path, mode: io.BytesIO(stat_data[path])):
        assert observer.session_leaders([111, 222]) == [222]
    with mock.patch('builtins.open', return_value=io.BytesIO(b'Name:\tpython\nNSpid:\t222\t4\n')):
        assert observer.namespace_pid(222) == 4
    for payload in (b'Name:\tpython\n', b'NSpid:\t222\tnope\n', b'NSpid:\t1\nNSpid:\t2\n'):
        with mock.patch('builtins.open', return_value=io.BytesIO(payload)):
            try:
                observer.namespace_pid(222)
                raise AssertionError('invalid NSpid accepted')
            except ValueError:
                pass
    for payload in (b'not-json\n', b'[]\n', b'{"loopback_only":true}\n'):
        try:
            observer.parse_ready(payload)
            raise AssertionError('malformed readiness accepted')
        except ValueError:
            pass
    for payload, expected in ((b'', TimeoutError), (b'partial', TimeoutError),
                              (b'one\ntwo\n', ValueError), (b'x' * 4097, ValueError)):
        read_fd, write_fd = os.pipe()
        try:
            if payload:
                os.write(write_fd, payload)
            with os.fdopen(read_fd, 'rb') as stream:
                try:
                    observer.read_ready(stream, 0.03)
                    raise AssertionError('invalid readiness accepted')
                except expected:
                    pass
        finally:
            os.close(write_fd)
    scan = mock.MagicMock()
    scan.__enter__.return_value = iter([types.SimpleNamespace(name=str(i)) for i in range(3)])
    with mock.patch.object(observer, 'MAX_PROC_ENTRIES', 2), mock.patch.object(observer.os, 'scandir', return_value=scan):
        try:
            observer.proc_entries()
            raise AssertionError('proc entry bound not enforced')
        except ValueError:
            pass
    with mock.patch.object(observer, 'proc_entries', return_value=['12']), \
            mock.patch.object(observer.pathlib.Path, 'stat', side_effect=PermissionError):
        for operation in (lambda: observer.matching_pids('token'), observer.owned_namespaces):
            try:
                operation()
                raise AssertionError('permission failure must make scan inconclusive')
            except PermissionError:
                pass
    with mock.patch.object(observer.os.path, 'isfile', return_value=False):
        assert observer.observe()['reason'] == 'unavailable'
    child = mock.Mock(pid=999)
    child.stdout = io.BytesIO(json.dumps({
        'readonly_write_rejected': True, 'docker_socket_unreachable': True,
        'resolver_config_absent': True, 'loopback_only': True,
        'network_connection_failed': True, 'dns_resolution_failed': True,
        'setsid_namespace_pid': 4,
    }).encode() + b'\n')
    child.poll.return_value = 0
    sentinel = mock.Mock(pid=998)
    sentinel.poll.return_value = None
    with tempfile.TemporaryDirectory() as root, \
            mock.patch.dict(os.environ, {'RUNNER_TEMP': root}), \
            mock.patch.object(observer.os.path, 'isfile', return_value=True), \
            mock.patch.object(observer.pathlib.Path, 'is_dir', return_value=True), \
            mock.patch.object(observer.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'bubblewrap 0.11.0\n')), \
            mock.patch.object(observer.subprocess, 'Popen', side_effect=[sentinel, child]) as popen, \
            mock.patch.object(observer, 'read_ready', return_value=child.stdout.getvalue()), \
            mock.patch.object(observer, 'matching_pids', side_effect=[[999, 111, 222], [], []]), \
            mock.patch.object(observer, 'session_leaders', return_value=[222]), \
            mock.patch.object(observer, 'namespace_pid', return_value=4), \
            mock.patch.object(observer.os, 'readlink', side_effect=lambda path: 'pid:[1]' if str(path) == '/proc/self/ns/pid' else 'pid:[2]'), \
            mock.patch.object(observer, 'owned_namespaces', return_value={'pid:[1]'}), \
            mock.patch.object(observer.os, 'kill') as kill:
        result = observer.observe()
        assert result['completed'] and result['descendants_disappeared']
        assert result['setsid_descendant_identified']
        assert result['unrelated_sentinel_survived'] and not result['cleanup_guaranteed']
        assert result['observed_namespace_disappeared'] and result['bubblewrap_version'] == 'bubblewrap 0.11.0'
        assert result['process_scan_scope'] == 'same-uid-only'
        assert result['network_failure_interpretation'] == 'inconclusive-without-external-positive-control'
        assert result['readonly_failure_interpretation'] == 'oserror-not-proof-of-mount'
        assert result['socket_failure_interpretation'] == 'unreachable-not-proof-of-absence'
        assert result['readonly_write_rejected'] and result['docker_socket_unreachable']
        args = popen.call_args_list[1].args[0]
        assert all(flag in args for flag in ('--unshare-pid', '--unshare-net', '--die-with-parent', '--new-session'))
        kill.assert_called_once_with(999, observer.signal.SIGKILL)
        sentinel.terminate.assert_called_once()
    for wrong_namespace in (False, True):
        child = mock.Mock(pid=999)
        child.poll.return_value = None
        sentinel = mock.Mock(pid=998)
        sentinel.poll.return_value = None
        readlink_calls = []
        def readlink(path):
            readlink_calls.append(str(path))
            if str(path) == '/proc/self/ns/pid':
                return 'pid:[1]'
            if wrong_namespace and str(path) == '/proc/222/ns/pid' and readlink_calls.count(str(path)) > 1:
                return 'pid:[3]'
            return 'pid:[2]'
        with tempfile.TemporaryDirectory() as root, \
                mock.patch.dict(os.environ, {'RUNNER_TEMP': root}), \
                mock.patch.object(observer.os.path, 'isfile', return_value=True), \
                mock.patch.object(observer.pathlib.Path, 'is_dir', return_value=True), \
                mock.patch.object(observer.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'bubblewrap 0.11.0\n')), \
                mock.patch.object(observer.subprocess, 'Popen', side_effect=[sentinel, child]), \
                mock.patch.object(observer, 'read_ready', return_value=json.dumps({
                    'readonly_write_rejected': True, 'docker_socket_unreachable': True,
                    'resolver_config_absent': True, 'loopback_only': True,
                    'network_connection_failed': True, 'dns_resolution_failed': True,
                    'setsid_namespace_pid': 4,
                }).encode() + b'\n'), \
                mock.patch.object(observer, 'matching_pids', return_value=[999, 111, 222]), \
                mock.patch.object(observer, 'session_leaders', return_value=[222]), \
                mock.patch.object(observer, 'namespace_pid', return_value=4 if wrong_namespace else 5), \
                mock.patch.object(observer.os, 'readlink', side_effect=readlink), \
                mock.patch.object(observer.os, 'kill') as kill:
            result = observer.observe()
            assert not result['completed'] and result['reason'] == 'setsid_descendant_not_identified'
            kill.assert_not_called()
            child.kill.assert_called_once()


if __name__ == '__main__':
    main()
