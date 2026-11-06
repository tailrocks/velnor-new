# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import subprocess
import tarfile
import tempfile
import time

root = Path(__file__).resolve().parents[1] / 'src'
toolchain = os.environ['VELNOR_SOURCE_TEST_TOOLCHAIN']
if os.geteuid() != 0 or not os.path.isabs(toolchain):
    raise RuntimeError('root and an explicit toolchain are required')

def load(name):
    spec = importlib.util.spec_from_file_location(name, root / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

bus = load('release_manager_bus')
lifecycle = load('release_manager_lifecycle')
executor = load('release_source_executor')

def modes(root_path, writable):
    for current, directories, files in os.walk(root_path):
        Path(current).chmod(0o755 if writable else 0o555)
        for name in files:
            path = Path(current) / name
            if not path.is_symlink():
                executable = bool(path.stat().st_mode & 0o111)
                path.chmod((0o755 if writable else 0o555) if executable else (0o644 if writable else 0o444))

def git(path, *args):
    return subprocess.run(['/usr/bin/git', '-C', str(path), *args], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout.strip()

def commit(path):
    git(path, 'add', '.')
    git(path, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
        'commit', '--quiet', '-m', 'synthetic')
    return git(path, 'rev-parse', 'HEAD')

def context_for(commit_sha, tools, version):
    value = {'identity': {'repository': 'fixture/example', 'repository_id': 2, 'owner_id': 3, 'run_id': 4, 'run_attempt': 1, 'workflow_sha': 'f' * 40, 'intent_sha256': 'e' * 64}, 'source_sha': commit_sha, 'rust_version': '1.99.1', 'manifest': 'Cargo.toml', 'source_files': inventory_from_git(source),
             'packages': [{'name': 'velnor_online_probe', 'version': version, 'manifest': None if version is None else 'Cargo.toml', 'features': []}],
             'tools': tools}
    descriptor = os.memfd_create('synthetic-context', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    try:
        os.write(descriptor, json.dumps(value, separators=(',', ':')).encode())
        fcntl.fcntl(descriptor, fcntl.F_ADD_SEALS,
                    fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        return executor.read_context(descriptor)
    finally:
        os.close(descriptor)

def inventory_from_git(source):
 data=subprocess.run(['/usr/bin/git','-C',str(source),'ls-files','--stage','-z'],check=True,stdout=subprocess.PIPE).stdout
 result=[]
 for entry in data.split(b'\0'):
  if not entry: continue
  header,path=entry.split(b'\t',1)
  mode,oid,stage=header.decode().split()
  if stage!='0' or mode not in ('100644','100755','120000'): raise RuntimeError('unsupported fixture entry')
  content=subprocess.run(['/usr/bin/git','-C',str(source),'cat-file','blob',oid],check=True,stdout=subprocess.PIPE).stdout
  kind={'type':'symlink','target':content.decode()} if mode=='120000' else {'type':'regular','executable':mode=='100755'}
  result.append({'path':path.decode(),'sha256':hashlib.sha256(content).hexdigest(),'kind':kind})
 return sorted(result,key=lambda item:item['path'])

def inventory_from_tree(source):
 result=[]
 for current,dirs,files in os.walk(source,topdown=True,followlinks=False):
  if Path(current)==source and '.git' in dirs: dirs.remove('.git')
  for name in dirs+files:
   path=Path(current)/name
   if path.is_symlink():
    target=os.readlink(path)
    content=target.encode()
    kind={'type':'symlink','target':target}
   elif path.is_file():
    content=path.read_bytes()
    kind={'type':'regular','executable':bool(path.stat().st_mode&0o111)}
   else: continue
   result.append({'path':str(path.relative_to(source)),'sha256':hashlib.sha256(content).hexdigest(),'kind':kind})
 return sorted(result,key=lambda item:item['path'])

def seal_context(value):
 descriptor=os.memfd_create('fixture-context',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
 try:
  os.write(descriptor,json.dumps(value,separators=(',',':')).encode())
  fcntl.fcntl(descriptor,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL)
  return executor.read_context(descriptor)
 finally: os.close(descriptor)


with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as temporary:
    directory = Path(temporary)
    directory.chmod(0o711)
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    socket_path = directory / 'external.sock'
    listener.bind(str(socket_path))
    os.chmod(socket_path, 0o777)
    listener.listen(1)
    host_sentinel = Path('/tmp') / ('velnor-host-sentinel-' + secrets.token_hex(8))
    host_sentinel.write_text('synthetic fixture')
    base = directory / 'base'
    (base / 'src').mkdir(parents=True)
    (base / 'Cargo.toml').write_text('[package]\nname="velnor_online_probe"\nversion="0.1.1"\n'
                                      'edition="2021"\ndescription="Synthetic fixture"\nlicense="MIT"\n'
                                      '[dependencies]\nvte="=0.15.0"\n')
    (base / 'src/lib.rs').write_text('pub fn fixture() -> u32 { 42 }\n')
    (base / 'build.rs').write_text('fn main() { for k in ["GIT_TOKEN","GH_TOKEN","GITHUB_TOKEN",'
                                  '"CARGO_REGISTRY_TOKEN","ACTIONS_ID_TOKEN_REQUEST_TOKEN",'
                                  '"ACTIONS_ID_TOKEN_REQUEST_URL"] { assert!(std::env::var_os(k).is_none()); } '
                                  'assert!(std::fs::read("/proc/'+str(os.getpid())+'/environ").is_err()); '
                                  'assert!(!std::path::Path::new("'+str(host_sentinel)+'").exists()); '
                                  'assert!(std::os::unix::net::UnixStream::connect("'+str(socket_path)+'").is_err()); }\n')
    cargo = str(Path(toolchain) / 'bin/cargo')
    env = {'PATH': str(Path(toolchain) / 'bin') + ':/usr/bin:/bin', 'HOME': str(directory),
           'CARGO_HOME': str(directory / 'lock-home'), 'RUSTC': str(Path(toolchain) / 'bin/rustc')}
    subprocess.run([cargo, 'generate-lockfile', '--manifest-path', str(base / 'Cargo.toml')],
                   cwd='/tmp', env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   check=True, timeout=120)
    subprocess.run(['/usr/bin/git', 'init', '--quiet', str(base)], check=True)
    good_sha = commit(base)
    git(base, 'checkout', '--quiet', '--detach', good_sha)
    print('staging synthetic online toolchain', flush=True)
    toolroot = directory / 'rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu'
    toolroot.parent.mkdir(parents=True)
    toolroot.parent.chmod(0o711)
    toolroot.parent.parent.chmod(0o711)
    shutil.copytree(toolchain, toolroot, symlinks=True)
    modes(toolroot, True)
    tools = {name: hashlib.sha256((toolroot / 'bin' / name).read_bytes()).hexdigest()
             for name in ('cargo', 'rustc', 'rustdoc')}
    helper = directory / 'source-helper.py'
    shutil.copyfile(root / 'release_supervisor_exec.py', helper)
    helper.chmod(0o555)
    supervisor = 'velnor-release-' + secrets.token_hex(16) + '.service'
    subprocess.run(['/usr/bin/systemd-run', '--unit=' + supervisor, '--property=Type=exec',
                    '/bin/sleep', '480'], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    manager = bus.Manager(str(helper), 180, bind_to=supervisor, deny_unix=True)
    source = directory / 'source'
    end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 420_000_000_000
    try:
        os.environ['GIT_TOKEN'] = 'fictional-fixture-token'
        os.environ['ACTIONS_ID_TOKEN_REQUEST_TOKEN'] = 'fictional-fixture-oidc'
        shutil.copytree(base, source, symlinks=True)
        modes(source, False)
        good = context_for(good_sha, tools, None)
        good_bound = context_for(good_sha, tools, '0.1.1')
        session = executor.SourceSession(manager, lifecycle, str(directory), good, end, (directory.stat().st_dev, directory.stat().st_ino))
        session.metadata()
        session.bind(good_bound)
        archive = session.package(0, True)
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as tar:
            assert tar.extractfile('velnor_online_probe-0.1.1/src/lib.rs').read() == b'pub fn fixture() -> u32 { 42 }\n'
            assert json.load(tar.extractfile('velnor_online_probe-0.1.1/.cargo_vcs_info.json'))['git']['sha1'] == good_sha
        print('locked real-registry package and credential isolation passed', flush=True)
        modes(source, True)
        lock = source / 'Cargo.lock'
        pattern = r'(\[\[package\]\]\s+name = "vte"\s+version = "0\.15\.0"\s+source = "[^"]+"\s+checksum = ")([a-f0-9]{64})(")'
        text = lock.read_text()
        match = re.search(pattern, text)
        if not match:
            raise AssertionError('missing pinned dependency checksum')
        bad_checksum = ('0' if match.group(2)[0] != '0' else '1') + match.group(2)[1:]
        lock.write_text(text[:match.start(2)] + bad_checksum + text[match.end(2):])
        checksum_sha = commit(source)
        modes(source, False)
        try:
            check_session = executor.SourceSession(manager, lifecycle, str(directory), context_for(checksum_sha, tools, None), end, (directory.stat().st_dev, directory.stat().st_ino))
            check_session.metadata()
            check_session.bind(context_for(checksum_sha, tools, '0.1.1'))
            check_session.package(0, True)
            raise AssertionError('corrupt dependency checksum accepted')
        except lifecycle.SupervisorError:
            pass
        print('corrupt dependency checksum rejected', flush=True)
        modes(source, True)
        lock.write_text(text)
        manifest = source / 'Cargo.toml'
        manifest.write_text(manifest.read_text().replace('vte="=0.15.0"', 'vte="=0.14.1"'))
        drift_sha = commit(source)
        modes(source, False)
        try:
            drift_session = executor.SourceSession(manager, lifecycle, str(directory), context_for(drift_sha, tools, None), end, (directory.stat().st_dev, directory.stat().st_ino))
            drift_session.metadata()
            drift_session.bind(context_for(drift_sha, tools, '0.1.1'))
            drift_session.package(0, True)
            raise AssertionError('lockfile drift accepted')
        except lifecycle.SupervisorError:
            pass
        print('lockfile drift rejected', flush=True)
        modes(source, True)
        shutil.rmtree(source)
        shutil.copytree(base, source, symlinks=True)
        modes(source, False)
        denied = bus.Manager(str(helper), 60, bind_to=supervisor, deny_network=True, deny_unix=True)
        try:
            try:
                denied_session = executor.SourceSession(denied, lifecycle, str(directory), good, end, (directory.stat().st_dev, directory.stat().st_ino))
                denied_session.metadata()
                denied_session.bind(good_bound)
                denied_session.package(0, True)
                raise AssertionError('package succeeded without dependency network')
            except lifecycle.SupervisorError:
                pass
        finally:
            denied.close()
        print('denied dependency download failed closed', flush=True)
    finally:
        os.environ.pop('GIT_TOKEN', None)
        os.environ.pop('ACTIONS_ID_TOKEN_REQUEST_TOKEN', None)
        manager.close()
        listener.close()
        host_sentinel.unlink(missing_ok=True)
        subprocess.run(['/usr/bin/systemctl', 'stop', supervisor], check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
