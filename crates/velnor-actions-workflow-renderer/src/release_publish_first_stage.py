# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import ast
import hashlib
import tempfile
import fcntl
import json
import os
import shutil
import struct
import sys
import time


def export_verified(entry, root, binary, digest, deadline, watch):
    return _export_verified(entry, root, binary, digest, deadline, watch, 'publish', 'publisher-entry')


def export_preflight_verified(entry, root, binary, digest, deadline, watch):
    return _export_verified(entry, root, binary, digest, deadline, watch, 'preflight', 'preflight-entry')


def _export_verified(entry, root, binary, digest, deadline, watch, operation, command):
    if (operation, command) not in (('publish', 'publisher-entry'), ('preflight', 'preflight-entry')) or watch.data['operation'] != operation:
        raise ValueError('invalid exporter role')
    work_deadline = deadline - entry['CLEANUP_RESERVE_NS']
    verified = entry['read_verified_binary'](binary, digest)
    os.close(verified)
    manager = entry['module']('velnor_export_manager', entry['MANAGER'])
    lifecycle = entry['module']('velnor_export_lifecycle', entry['LIFECYCLE'])
    helper = entry['stage_fixed'](root, 'release_supervisor_exec.py', entry['decoded'](entry['HELPER']), 0o500)
    output = os.memfd_create('velnor-publisher-entry', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    try:
        os.ftruncate(output, 512 * 1024)
        fcntl.fcntl(output, fcntl.F_ADD_SEALS, fcntl.F_SEAL_GROW)
        env = {'PATH': '/usr/bin:/bin', 'HOME': root, 'TMPDIR': root, 'RUNNER_TEMP': root, 'LANG': 'C'}
        body = json.dumps({'program': binary,
                           'args': ['crate-release', command, '--timeout-seconds', '1800'],
                           'cwd': root, 'env': env, 'stdin': ''}, separators=(',', ':')).encode()
        namespace = os.stat('/proc/self/ns/time').st_ino
        frame = struct.pack('>QQI', work_deadline, namespace, len(body)) + body
        service = manager.Manager(helper, 1800, watch.authority, root_bootstrap=True)
        try:
            lifecycle.supervise(service, frame, work_deadline, output_fd=output)
        finally:
            service.close()
        size = os.lseek(output, 0, os.SEEK_CUR)
        if (not 0 < size <= 512 * 1024 or os.fstat(output).st_size != 512 * 1024
                or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline):
            raise ValueError('publisher export invalid')
        exported = os.pread(output, size, 0)
        names = ('WATCH_CONTRACT', 'WATCH_OPERATION', 'WATCH_BOOTSTRAP')
        assignments = {name: [] for name in names}
        for node in ast.walk(ast.parse(exported)):
            if isinstance(node, (ast.Assign, ast.AnnAssign, ast.AugAssign, ast.NamedExpr)):
                targets = node.targets if isinstance(node, ast.Assign) else [node.target]
                for target in targets:
                    if isinstance(target, ast.Name) and target.id in assignments:
                        assignments[target.id].append(node)
        if any(len(nodes) != 1 or not isinstance(nodes[0], ast.Assign) or len(nodes[0].targets) != 1
               or not isinstance(nodes[0].value, ast.Constant) or type(nodes[0].value.value) is not str
               for nodes in assignments.values()):
            raise ValueError('exported watch contract unavailable')
        values = {name: nodes[0].value.value for name, nodes in assignments.items()}
        if (values['WATCH_CONTRACT'] != 'velnor.root-watch.v1' or values['WATCH_OPERATION'] != operation
                or hashlib.sha256(base64.b64decode(values['WATCH_BOOTSTRAP'], validate=True)).hexdigest()
                   != hashlib.sha256(entry['decoded'](entry['WATCH_BOOTSTRAP'])).hexdigest()):
            raise ValueError('exported watch contract mismatch')
        os.ftruncate(output, size)
        fcntl.fcntl(output, fcntl.F_ADD_SEALS,
                    fcntl.F_SEAL_WRITE | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        os.lseek(output, 0, os.SEEK_SET)
        verified = entry['read_verified_binary'](binary, digest)
        os.close(verified)
        return output
    except BaseException:
        os.close(output)
        raise


def main():
    if len(sys.argv) != 5:
        raise ValueError('invalid publisher arguments')
    mode, version, source_sha, manifest_sha = sys.argv[1:]
    entry_source = bundle['entry']
    for key, name in (('bootstrap', 'BOOTSTRAP'), ('manager', 'MANAGER'),
                      ('lifecycle', 'LIFECYCLE'), ('watch_admission', 'WATCH_ADMISSION'), ('helper', 'HELPER'), ('bootstrap_watch', 'WATCH_BOOTSTRAP'),
                      ('watch_program', 'WATCH_PROGRAM'), ('claim', 'WATCH_CLAIM')):
        entry_source = entry_source.replace('__VELNOR_' + name + '_PAYLOAD__',
            base64.b64encode(bundle[key].encode()).decode())
    entry_source = entry_source.replace('__VELNOR_JOB_TIMEOUT_SECONDS__', '1800')
    entry = {'__name__': 'velnor_first_stage'}
    exec(compile(entry_source, '<verified-publisher-bootstrap>', 'exec'), entry)
    if (os.geteuid() != 0 or sys.platform != 'linux' or os.uname().machine != 'x86_64'
            or mode not in ('trusted-publishing', 'bootstrap')):
        raise ValueError('invalid publisher host')
    context = entry['read_context'](mode)
    budget = os.environ.get('VELNOR_RELEASE_BUDGET')
    deadline = entry['bound_deadline'](context, budget)
    import re
    if (not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version)
            or not re.fullmatch(r'[a-f0-9]{40}', source_sha)
            or not re.fullmatch(r'[a-f0-9]{64}', manifest_sha)):
        raise ValueError('invalid publisher pin')
    root = tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run')
    manager = entry['module']('velnor_stage_manager', entry['MANAGER'])
    lifecycle = entry['module']('velnor_stage_lifecycle', entry['LIFECYCLE'])
    watchdog = entry['module']('velnor_stage_watch', entry['WATCH_BOOTSTRAP'])
    watch_root = tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run')
    watch = watchdog.start(manager, lifecycle, watch_root, deadline, 'publish',
                           entry['decoded'](entry['WATCH_PROGRAM']), entry['decoded'](entry['CLAIM']))
    output = None
    try:
        binary, digest = entry['prepare_binary'](context, version, source_sha, manifest_sha, deadline, root, watch)
        output = export_verified(entry, root, binary, digest, deadline, watch)
        shutil.rmtree(root)
        env = dict(context)
        env.update({'PATH': '/usr/bin:/bin', 'HOME': '/nonexistent', 'LANG': 'C',
                    'VELNOR_RELEASE_BUDGET': budget})
        os.set_inheritable(output, True)
        env.update(watch.prepare_exec_environment())
        os.execve('/usr/bin/python3', ['/usr/bin/python3', '-I', '/proc/self/fd/' + str(output),
                                     mode, version, source_sha, manifest_sha], env)
    finally:
        if output is not None:
            os.close(output)
        watch.close(deadline)


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('publisher first stage rejected') from None
