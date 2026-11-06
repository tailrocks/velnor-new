# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import fcntl
import json
import os
import shutil
import struct
import sys
import time


def export_verified(entry, root, binary, digest, deadline):
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
                           'args': ['crate-release', 'publisher-entry', '--timeout-seconds', '1800'],
                           'cwd': root, 'env': env, 'stdin': ''}, separators=(',', ':')).encode()
        namespace = os.stat('/proc/self/ns/time').st_ino
        frame = struct.pack('>QQI', deadline, namespace, len(body)) + body
        service = manager.Manager(helper, 1800, root_bootstrap=True)
        try:
            lifecycle.supervise(service, frame, deadline, output_fd=output)
        finally:
            service.close()
        size = os.lseek(output, 0, os.SEEK_CUR)
        if (not 0 < size <= 512 * 1024 or os.fstat(output).st_size != 512 * 1024
                or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline):
            raise ValueError('publisher export invalid')
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
                      ('lifecycle', 'LIFECYCLE'), ('helper', 'HELPER')):
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
    root, binary, digest = entry['prepare_binary'](context, version, source_sha, manifest_sha, deadline)
    output = export_verified(entry, root, binary, digest, deadline)
    try:
        shutil.rmtree(root)
        env = dict(context)
        env.update({'PATH': '/usr/bin:/bin', 'HOME': '/nonexistent', 'LANG': 'C',
                    'VELNOR_RELEASE_BUDGET': budget})
        os.set_inheritable(output, True)
        os.execve('/usr/bin/python3', ['/usr/bin/python3', '-I', '/proc/self/fd/' + str(output),
                                     mode, version, source_sha, manifest_sha], env)
    finally:
        os.close(output)


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('publisher first stage rejected') from None
