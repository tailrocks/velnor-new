# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl, hashlib, importlib.util, io, json, os, pathlib, secrets, shutil, subprocess, tarfile, tempfile, time
modules=pathlib.Path(os.environ.get('VELNOR_SOURCE_TEST_RENDERER') or pathlib.Path(__file__).resolve().parents[1]/'src')
def load(name):
 spec=importlib.util.spec_from_file_location(name, modules/(name+'.py'))
 obj=importlib.util.module_from_spec(spec)
 spec.loader.exec_module(obj)
 return obj
bus=load('release_manager_bus')
lifecycle=load('release_manager_lifecycle')
executor=load('release_source_executor')
fault=os.environ.get('VELNOR_SOURCE_TEST_FAULT','')
if fault not in ('', 'git-lock', 'outside-workspace'):
 raise RuntimeError('invalid fixture fault')
unit='velnor-release-'+secrets.token_hex(16)+'.service'
subprocess.run(['/usr/bin/systemd-run','--quiet','--unit='+unit,'--property=Type=exec','/bin/sleep','500'],check=True)
helper='/run/velnor-nested-helper-'+secrets.token_hex(12)
try:
 shutil.copyfile(modules/'release_supervisor_exec.py',helper)
 os.chmod(helper,0o555)
 with tempfile.TemporaryDirectory(dir='/run',prefix='velnor-publisher-') as tmp:
  root=pathlib.Path(tmp)
  root.chmod(0o711)
  source=root/'source'
  nested=source/'nested'
  (nested/'src').mkdir(parents=True)
  (source/'Cargo.toml').write_text('[workspace]\nmembers=[]\n')
  (source/'Cargo.lock').write_text('version=4\n[[package]]\nname="foreign"\nversion="1.0.0"\nsource="git+https://example.invalid/foreign#abc"\n')
  (nested/'Cargo.toml').write_text('[workspace]\n[package]\nname="fixture"\nversion="0.1.0"\nedition="2024"\n'+('workspace="../../outside"\n' if fault=='outside-workspace' else ''))
  (nested/'Cargo.lock').write_text('version=4\n[[package]]\nname="fixture"\nversion="0.1.0"\n'+('source="git+https://example.invalid/repo#abc"\n' if fault=='git-lock' else ''))
  (nested/'src/lib.rs').write_text('pub fn example() {}\n')
  def git(*args):
   return subprocess.run(['/usr/bin/git','-C',str(source),*args],check=True,capture_output=True,text=True).stdout.strip()
  git('init','-q')
  git('add','-A')
  git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture')
  commit=git('rev-parse','HEAD')
  git('checkout','--detach','-q',commit)
  inventory=[]
  for name in sorted(git('ls-files').splitlines()):
   p=source/name
   inventory.append({'path':name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'kind':{'type':'regular','executable':bool(p.stat().st_mode&0o111)}})
  for current,dirs,files in os.walk(source):
   pathlib.Path(current).chmod(0o555)
   for name in files:
    path=pathlib.Path(current)/name
    if not path.is_symlink(): path.chmod(0o555 if path.stat().st_mode&0o111 else 0o444)
  toolroot=root/'rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu'
  toolroot.parent.mkdir(parents=True)
  toolroot.parent.chmod(0o711)
  toolroot.parent.parent.chmod(0o711)
  shutil.copytree(os.environ['VELNOR_SOURCE_TEST_TOOLCHAIN'],toolroot,symlinks=True)
  for current,dirs,files in os.walk(toolroot):
   pathlib.Path(current).chmod(0o755)
   for name in files:
    path=pathlib.Path(current)/name
    if not path.is_symlink(): path.chmod(0o755 if path.stat().st_mode&0o111 else 0o644)
  gitdir=root/'git'
  gitdir.mkdir()
  shutil.copyfile(os.environ['VELNOR_TEST_PINNED_GIT'],gitdir/'git')
  shutil.copyfile(os.environ['VELNOR_TEST_PINNED_GIT_HTTP'],gitdir/'git-remote-http')
  (gitdir/'git-remote-https').symlink_to('git-remote-http')
  (gitdir/'git').chmod(0o555)
  (gitdir/'git-remote-http').chmod(0o555)
  gitdir.chmod(0o555)
  semver=root/'semver'
  semver.mkdir()
  shutil.copyfile(os.environ['VELNOR_TEST_SEMVER'],semver/'cargo-semver-checks')
  (semver/'cargo-semver-checks').chmod(0o555)
  semver.chmod(0o555)
  sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
  tools={name:sha(toolroot/'bin'/name) for name in ('cargo','rustc','rustdoc')}
  tools.update({'semver':sha(semver/'cargo-semver-checks'),'git':sha(gitdir/'git'),'git_http':sha(gitdir/'git-remote-http'),'cc':'a'*64,'release_plz':'b'*64})
  value={'identity':{'repository':'fixture/example','repository_id':2,'owner_id':3,'run_id':4,'run_attempt':1,'workflow_sha':'f'*40,'intent_sha256':'e'*64},'source_sha':commit,'rust_version':'1.99.1','manifest':'nested/Cargo.toml','authentication':'trusted-publishing','source_files':inventory,'packages':[{'name':'fixture','version':None,'manifest':None,'features':[],'upload_config_sha256':'d'*64}],'tools':tools}
  descriptor=os.memfd_create('fixture-context',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
  os.write(descriptor,json.dumps(value,separators=(',',':')).encode())
  fcntl.fcntl(descriptor,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL)
  context=executor.read_context(descriptor)
  os.close(descriptor)
  end=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+300_000_000_000
  manager=bus.Manager(helper,240,bind_to=unit,deny_unix=True,deny_network=True)
  manager.subscribe(end)
  locator=bus.Manager(helper,240,bind_to=unit,deny_unix=True,deny_network=True)
  locator.subscribe(end)
  try:
   session=executor.SourceSession(manager,locator,lifecycle,str(root),context,end,(root.stat().st_dev,root.stat().st_ino))
   try:
    result=json.loads(session.metadata())
   except executor.SourceExecutorError as error:
    if not fault: raise
    expected={'git-lock':'dependency source unsupported','outside-workspace':'source path unsupported'}
    assert str(error)==expected[fault], (fault,str(error))
    print('invalid workspace rejected through supervised source session',flush=True)
   else:
    if fault: raise AssertionError('unsafe workspace accepted')
    assert result['workspace_root']==str(nested)
    bound=dict(value,packages=[dict(value['packages'][0],version='0.1.0',manifest='nested/Cargo.toml')])
    descriptor=os.memfd_create('fixture-bound-context',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
    os.write(descriptor,json.dumps(bound,separators=(',',':')).encode())
    fcntl.fcntl(descriptor,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL)
    session.bind(executor.read_context(descriptor))
    os.close(descriptor)
    archive=session.package(0,False)
    with tarfile.open(fileobj=io.BytesIO(archive),mode='r:gz') as package:
     assert 'fixture-0.1.0/src/lib.rs' in package.getnames()
    print('nested workspace admitted and packaged through supervised source session',flush=True)
  finally:
   locator.close()
   manager.close()
finally:
 try:
  subprocess.run(['/usr/bin/systemctl','stop',unit],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True)
 finally:
  pathlib.Path(helper).unlink(missing_ok=True)
