import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import ast, base64, fcntl, hashlib, importlib.util, json, os, pathlib, shutil, struct, subprocess, sys, tempfile, time
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from release_watch_fixture import RootWatch
root = pathlib.Path(sys.argv[1]).resolve(strict=True)
spec=importlib.util.spec_from_file_location('first_stage', root / 'release_publish_first_stage.py')
stage=importlib.util.module_from_spec(spec); spec.loader.exec_module(stage)
with open(sys.argv[3], 'rb') as stream:
    raw=stream.read(512*1024+1)
assert 0<len(raw)<=512*1024
bundle=json.loads(raw)
entry=stage.bootstrap_entry(bundle)
manager=entry['module']('fixture_manager',entry['MANAGER'])
lifecycle=entry['module']('fixture_lifecycle',entry['LIFECYCLE'])
with RootWatch(manager,lifecycle,root) as watch, tempfile.TemporaryDirectory(prefix='velnor-publisher-', dir='/run') as private:
    binary=pathlib.Path(private)/'generator'; shutil.copyfile(sys.argv[2], binary); binary.chmod(0o500)
    digest=hashlib.sha256(binary.read_bytes()).hexdigest(); deadline=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+120_000_000_000
    fd=stage.export_verified(entry, private, str(binary), digest, deadline, watch.watch)
    try:
        actual=os.read(fd,512*1024+1)
        direct=subprocess.run([str(binary),'crate-release','publisher-entry','--timeout-seconds','1800'],env={'PATH':'/usr/bin:/bin','HOME':private,'LANG':'C'},stdout=subprocess.PIPE,check=True).stdout
        assert actual==direct and 0<len(actual)<=512*1024
        assignments = {node.targets[0].id: ast.literal_eval(node.value)
                       for node in ast.parse(actual).body
                       if isinstance(node, ast.Assign) and len(node.targets) == 1
                       and isinstance(node.targets[0], ast.Name)
                       and node.targets[0].id in {'MANAGER', 'WATCH_ADMISSION', 'MANAGER_POLICY_HOST', 'MANAGER_POLICY_FILES', 'MANAGER_POLICY', 'LIFECYCLE', 'HELPER', 'SUPERVISOR', 'WATCH_BOOTSTRAP', 'WATCH_PROGRAM', 'CLAIM'}}
        for key, filename in [('MANAGER', 'release_manager_bus.py'), ('WATCH_ADMISSION', 'release_watch_admission.py'), ('MANAGER_POLICY_HOST', 'release_manager_policy_host.py'), ('MANAGER_POLICY_FILES', 'release_manager_policy_files.py'), ('MANAGER_POLICY', 'release_manager_policy.py'), ('LIFECYCLE', 'release_manager_lifecycle.py'), ('HELPER', 'release_supervisor_exec.py'), ('WATCH_BOOTSTRAP', 'release_bootstrap_watch.py'), ('WATCH_PROGRAM', 'release_launcher_watch.py'), ('CLAIM', 'release_supervisor_claim.py')]:
            assert base64.b64decode(assignments[key], validate=True) == (root / filename).read_bytes()
        embedded = json.loads(base64.b64decode(assignments['SUPERVISOR'], validate=True))
        for key, filename in [('coordinator','release_coordinator_exec.py'), ('retained','release_retained_parent.py'), ('source','release_source_executor.py'), ('channel','release_source_channel.py'), ('upload','release_upload_operation.py'), ('broker','release_forge_broker.py')]:
            assert base64.b64decode(embedded[key], validate=True) == (root / filename).read_bytes()
        try: os.write(fd,b'x')
        except OSError: pass
        else: raise AssertionError('export memfd remained writable')
        print('real managed exporter output matches and memfd is sealed',flush=True)
    finally: os.close(fd)
    try: stage.export_verified(entry, private, str(binary), '0'*64, deadline, watch.watch)
    except ValueError: pass
    else: raise AssertionError('mismatched binary accepted')

    manager=entry['module']('overflow_manager',entry['MANAGER'])
    lifecycle=entry['module']('overflow_lifecycle',entry['LIFECYCLE'])
    helper=str(pathlib.Path(private)/'release_supervisor_exec.py')
    fd=os.memfd_create('overflow',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
    try:
        os.ftruncate(fd,512*1024); fcntl.fcntl(fd,fcntl.F_ADD_SEALS,fcntl.F_SEAL_GROW)
        body=json.dumps({'program':'/usr/bin/head','args':['-c','524289','/dev/zero'],'cwd':private,'env':{'PATH':'/usr/bin:/bin','HOME':private},'stdin':''},separators=(',',':')).encode()
        frame=struct.pack('>QQI',deadline,os.stat('/proc/self/ns/time').st_ino,len(body))+body
        service=manager.Manager(helper,120,watch.authority,root_bootstrap=True)
        try:
            try: lifecycle.supervise(service,frame,deadline,output_fd=fd)
            except lifecycle.SupervisorError as error:
                assert str(error) == 'unit failed after verified cleanup', str(error)
            else: raise AssertionError('overflowing exporter succeeded')
        finally: service.close()
        assert os.fstat(fd).st_size==512*1024
        print('real manager rejects output beyond sealed capacity',flush=True)
    finally: os.close(fd)

    pid_file = pathlib.Path(private) / 'descendant.pid'
    code = ("import os,signal,sys,time\n"
            "child=os.fork()\n"
            "if child == 0:\n"
            " os.setsid()\n signal.signal(signal.SIGTERM, signal.SIG_IGN)\n time.sleep(120)\n"
            "else:\n open(sys.argv[1], 'w').write(str(child))\n time.sleep(120)\n")
    timeout = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 30_000_000_000
    body = json.dumps({'program':'/usr/bin/python3','args':['-I','-S','-c',code,str(pid_file)],
                       'cwd':private,'env':{'PATH':'/usr/bin:/bin','HOME':private},'stdin':''},
                      separators=(',',':')).encode()
    frame=struct.pack('>QQI',timeout,os.stat('/proc/self/ns/time').st_ino,len(body))+body
    service=manager.Manager(helper,40,watch.authority,root_bootstrap=True)
    try:
        try: lifecycle.supervise(service,frame,timeout)
        except lifecycle.SupervisorError as error:
            assert str(error) == 'unit failed after verified cleanup', str(error)
        else: raise AssertionError('timed out exporter succeeded')
    finally: service.close()
    child=int(pid_file.read_text())
    assert not pathlib.Path('/proc',str(child)).exists(), 'descendant survived timeout'
    print('real manager reaped detached descendant after timeout',flush=True)
