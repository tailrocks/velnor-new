# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import array, struct, threading
import fcntl, hashlib, importlib.util, io, json, os, pathlib, secrets, shutil, socket, subprocess, tarfile, tempfile, time
root=pathlib.Path(__file__).resolve().parents[1]/'src'
def load(name):
 spec=importlib.util.spec_from_file_location(name,root/(name+'.py'))
 obj=importlib.util.module_from_spec(spec)
 spec.loader.exec_module(obj)
 return obj
import sys
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parent))
from release_watch_fixture import RootWatch
bus=load('release_manager_bus')
lifecycle=load('release_manager_lifecycle')
executor=load('release_source_executor')
channel=load('release_source_channel')
toolchain=os.environ['VELNOR_SOURCE_TEST_TOOLCHAIN']
if os.geteuid()!=0 or not os.path.isabs(toolchain):
 raise RuntimeError('root and an explicit toolchain are required')
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
  if pathlib.Path(current)==source and '.git' in dirs: dirs.remove('.git')
  for name in dirs+files:
   path=pathlib.Path(current)/name
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


with tempfile.TemporaryDirectory(dir='/run',prefix='velnor-publisher-') as tmp:
 directory=pathlib.Path(tmp)
 directory.chmod(0o711)
 listener=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
 socket_path=directory/'external.sock'
 listener.bind(str(socket_path))
 os.chmod(socket_path,0o777)
 listener.listen(1)
 tcp_listener=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
 tcp_listener.bind(('127.0.0.1',0))
 tcp_listener.listen(2)
 tcp_listener.settimeout(3)
 port=tcp_listener.getsockname()[1]
 source=directory/'source'
 (source/'src').mkdir(parents=True)
 (source/'Cargo.toml').write_text('[package]\nname="velnor_executor_probe"\nversion="0.1.1"\nedition="2021"\ndescription="Synthetic fixture"\nlicense="MIT"\n[workspace]\nmembers=["member"]\n')
 (source/'Cargo.lock').write_text('version = 4\n\n[[package]]\nname = "other_probe"\nversion = "0.1.1"\n\n[[package]]\nname = "velnor_executor_probe"\nversion = "0.1.1"\n')
 (source/'member/src').mkdir(parents=True)
 (source/'member/Cargo.toml').write_text('[package]\nname="other_probe"\nversion="0.1.1"\nedition="2021"\n')
 (source/'member/src/lib.rs').write_text('pub fn other() {}\n')
 original=b'pub fn fixture() -> u32 { 42 }\n'
 (source/'src/lib.rs').write_bytes(original)
 (source/'build.rs').write_text('use std::io::Write; fn main() { assert!(std::os::unix::net::UnixStream::connect(\"'+str(socket_path)+'\").is_err()); let mut s=std::net::TcpStream::connect((\"127.0.0.1\",'+str(port)+')).unwrap(); s.write_all(b\"v\").unwrap(); }\n')
 subprocess.run(['/usr/bin/git','init','--quiet',str(source)],check=True)
 subprocess.run(['/usr/bin/git','-C',str(source),'add','.'],check=True)
 subprocess.run(['/usr/bin/git','-C',str(source),'-c','user.name=Fixture','-c','user.email=fixture@example.invalid',
                 'commit','--quiet','-m','fixture'],check=True)
 commit=subprocess.run(['/usr/bin/git','-C',str(source),'rev-parse','HEAD'],check=True,stdout=subprocess.PIPE,text=True).stdout.strip()
 subprocess.run(['/usr/bin/git','-C',str(source),'checkout','--quiet','--detach',commit],check=True)
 for current,dirs,files in os.walk(source):
  pathlib.Path(current).chmod(0o555)
  for name in files:
   path=pathlib.Path(current)/name
   if not path.is_symlink(): path.chmod(0o555 if path.stat().st_mode & 0o111 else 0o444)
 print('staging synthetic pinned toolchain',flush=True)
 toolroot=directory/'rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu'
 toolroot.parent.mkdir(parents=True)
 toolroot.parent.chmod(0o711)
 toolroot.parent.parent.chmod(0o711)
 shutil.copytree(toolchain,toolroot,symlinks=True)
 for current,dirs,files in os.walk(toolroot):
  pathlib.Path(current).chmod(0o755)
  for name in files:
   path=pathlib.Path(current)/name
   if not path.is_symlink(): path.chmod(0o755 if path.stat().st_mode & 0o111 else 0o644)
 tools={name:hashlib.sha256((toolroot/'bin'/name).read_bytes()).hexdigest() for name in ('cargo','rustc','rustdoc')}
 helper=directory/'source-helper.py'
 shutil.copyfile(root/'release_supervisor_exec.py',helper)
 helper.chmod(0o555)
 value={'identity': {'repository': 'fixture/example', 'repository_id': 2, 'owner_id': 3, 'run_id': 4, 'run_attempt': 1, 'workflow_sha': 'f' * 40, 'intent_sha256': 'e' * 64}, 'source_sha':commit,'rust_version':'1.99.1','manifest':'Cargo.toml','source_files':inventory_from_git(source),'packages':[{'name':'velnor_executor_probe','version':'0.1.1','manifest':'Cargo.toml','features':[]}],'tools':tools}
 context_fd=os.memfd_create('admitted-context',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
 os.write(context_fd,json.dumps(value,separators=(',',':')).encode())
 try:
  executor.read_context(context_fd)
  raise AssertionError('unsealed context accepted')
 except executor.SourceExecutorError: pass
 fcntl.fcntl(context_fd,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL)
 context=executor.read_context(context_fd)
 def snapshot():
  result={}
  for current,dirs,files in os.walk(source,followlinks=False):
   for name in dirs+files:
    path=pathlib.Path(current)/name
    info=path.lstat()
    key=str(path.relative_to(source))
    if path.is_symlink(): content=('link',os.readlink(path))
    elif path.is_file(): content=('file',hashlib.sha256(path.read_bytes()).hexdigest())
    else: content=('dir',None)
    result[key]=(info.st_mode & 0o777,content)
  return result
 before=snapshot()
 watch=RootWatch(bus,lifecycle,root)
 manager=bus.Manager(str(helper),60,watch.authority,deny_unix=True)
 try:
  end=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+120_000_000_000
  initial=seal_context(dict(value,packages=[dict(value['packages'][0],version=None,manifest=None)]))
  session=executor.SourceSession(manager,lifecycle,str(directory),initial,end,(directory.stat().st_dev,directory.stat().st_ino))
  try:
   session.package(0,True)
   raise AssertionError('unbound package accepted')
  except executor.SourceExecutorError: pass
  metadata=session.metadata()
  parsed=json.loads(metadata)
  assert any(item['name']=='velnor_executor_probe' for item in parsed['packages'])
  for field, replacement in {
      'repository': 'fixture/other', 'repository_id': 22, 'owner_id': 33,
      'run_id': 44, 'run_attempt': 2, 'workflow_sha': 'a' * 40, 'intent_sha256': 'b' * 64,
  }.items():
   changed=seal_context(dict(value,identity=dict(value['identity'],**{field:replacement})))
   try:
    session.bind(changed)
    raise AssertionError('changed source identity accepted: '+field)
   except executor.SourceExecutorError: pass
  changed_rust=seal_context(dict(value,rust_version='1.98.0'))
  try:
   session.bind(changed_rust)
   raise AssertionError('changed Rust version accepted')
  except executor.SourceExecutorError: pass
  for repository in ('-bad/example', 'bad--owner/example', 'good/.', 'good/..', 'good/example.git', 'a'*40+'/example', 'good/'+'a'*101):
   try:
    seal_context(dict(value,identity=dict(value['identity'],repository=repository)))
    raise AssertionError('invalid repository identity accepted')
   except executor.SourceExecutorError: pass
  for field in ('repository_id','owner_id','run_id','run_attempt'):
   try:
    seal_context(dict(value,identity=dict(value['identity'],**{field:True})))
    raise AssertionError('boolean source identity accepted: '+field)
   except executor.SourceExecutorError: pass
  wrong=seal_context(dict(value,packages=[dict(value['packages'][0],version='0.1.2')]))
  try:
   session.bind(wrong)
   raise AssertionError('mismatched metadata version accepted')
  except executor.SourceExecutorError: pass
  wrong_manifest=seal_context(dict(value,packages=[dict(value['packages'][0],manifest='member/Cargo.toml')]))
  try:
   session.bind(wrong_manifest)
   raise AssertionError('mismatched same-version manifest accepted')
  except executor.SourceExecutorError: pass
  try:
   executor.SourceSession(manager,lifecycle,str(directory),context,end,(directory.stat().st_dev,directory.stat().st_ino))
   raise AssertionError('prepopulated initial package accepted')
  except executor.SourceExecutorError: pass
  try:
   executor.SourceSession(manager,lifecycle,str(directory),initial,time.clock_gettime_ns(time.CLOCK_BOOTTIME)-1,(directory.stat().st_dev,directory.stat().st_ino))
   raise AssertionError('expired session accepted')
  except executor.SourceExecutorError: pass
  session.bind(context)
  moved=directory.with_name(directory.name+'-moved')
  directory.rename(moved)
  directory.mkdir(mode=0o711)
  try:
   try:
    session.package(0,False)
    raise AssertionError('replaced view accepted')
   except executor.SourceExecutorError as error:
    assert str(error)=='source view changed'
  finally:
   directory.rmdir()
   moved.rename(directory)
  assert session.metadata()==metadata
  archive=session.package(0,True)
  connection,_=tcp_listener.accept()
  with connection: assert connection.recv(1)==b'v'
  unverified=session.package(0,False)
  assert unverified==archive
  client,server=socket.socketpair()
  errors=[]
  def serve_channel():
   try: channel.serve(server,manager,lifecycle,executor,str(directory),end,(directory.stat().st_dev,directory.stat().st_ino))
   except BaseException as error: errors.append(error)
   finally: server.close()
  worker=threading.Thread(target=serve_channel)
  worker.start()
  client.settimeout(60)
  def read_bytes(count):
   result=b''
   while len(result)<count:
    part=client.recv(count-len(result))
    if not part: raise AssertionError('source channel closed')
    result+=part
   return result
  def call(sequence,operation,descriptor=None,index=None):
   request={'seq':sequence,'op':operation}
   if index is not None: request['index']=index
   data=json.dumps(request,separators=(',',':')).encode()
   client.sendall(struct.pack('>I',len(data))+data)
   if descriptor is not None:
    client.sendmsg([b'C'],[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[descriptor]))])
   received,size=struct.unpack('>QI',read_bytes(12))
   assert received==sequence
   return read_bytes(size)
  def context_descriptor(raw):
   descriptor=os.memfd_create('fixture-context',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
   os.write(descriptor,json.dumps(raw,separators=(',',':')).encode())
   fcntl.fcntl(descriptor,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL)
   return descriptor
  try:
   first_fd=context_descriptor(dict(value,packages=[dict(value['packages'][0],version=None,manifest=None)]))
   try: assert call(1,'init',first_fd)==b''
   finally: os.close(first_fd)
   assert json.loads(call(2,'metadata'))['workspace_root']==str(source)
   bound_fd=context_descriptor(value)
   try: assert call(3,'bind',bound_fd)==b''
   finally: os.close(bound_fd)
   assert call(4,'package',index=0)==archive
   assert call(5,'close')==b''
  finally:
   client.close()
   worker.join(timeout=10)
  assert not worker.is_alive() and not errors, errors
  tcp_listener.settimeout(.5)
  try:
   duplicate,_=tcp_listener.accept()
   duplicate.close()
   raise AssertionError('repair package unexpectedly ran build script')
  except TimeoutError: pass
  with tarfile.open(fileobj=io.BytesIO(archive),mode='r:gz') as tar:
   assert tar.extractfile('velnor_executor_probe-0.1.1/src/lib.rs').read()==original
   assert json.load(tar.extractfile('velnor_executor_probe-0.1.1/.cargo_vcs_info.json'))['git']['sha1']==commit
  try:
   session.package(1,True)
   raise AssertionError('invalid index accepted')
  except executor.SourceExecutorError: pass
  def rejected(reason):
   try:
    fixture_context=seal_context(dict(value,source_files=inventory_from_tree(source)))
    executor._source_operation(manager,lifecycle,str(directory),fixture_context,'metadata',None,end)
    raise AssertionError(reason+' accepted')
   except executor.SourceExecutorError as error:
    assert str(error)==reason, (reason,str(error))
  source.chmod(0o755)
  config=source/'.cargo'
  config.mkdir()
  (config/'config.toml').write_text('[build]\nrustc-wrapper="/nonexistent/velnor-poison-wrapper"\n')
  (config/'config.toml').chmod(0o444)
  config.chmod(0o555)
  source.chmod(0o555)
  try: rejected('project cargo config unsupported')
  finally:
   source.chmod(0o755)
   config.chmod(0o755)
   shutil.rmtree(config)
   source.chmod(0o555)
  lock=source/'Cargo.lock'
  lock_data=lock.read_bytes()
  lock.chmod(0o644)
  lock.write_bytes(lock_data+b'\n[[package]]\nname="foreign"\nversion="1.0.0"\nsource="git+https://example.invalid/repo#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"\n')
  lock.chmod(0o444)
  try: rejected('dependency source unsupported')
  finally:
   lock.chmod(0o644)
   lock.write_bytes(lock_data)
   lock.chmod(0o444)
  lock.chmod(0o644)
  lock.write_bytes(lock_data+b'\n[[package]]\nname="foreign"\nversion="1.0.0"\nsource="registry+https://example.invalid/index"\nchecksum="'+b'a'*64+b'"\n')
  lock.chmod(0o444)
  try: rejected('dependency source unsupported')
  finally:
   lock.chmod(0o644)
   lock.write_bytes(lock_data)
   lock.chmod(0o444)
  manifest=source/'Cargo.toml'
  manifest_data=manifest.read_bytes()
  manifest.chmod(0o644)
  manifest.write_bytes(manifest_data+b'\n[patch.crates-io]\n')
  manifest.chmod(0o444)
  try: rejected('manifest patch unsupported')
  finally:
   manifest.chmod(0o644)
   manifest.write_bytes(manifest_data)
   manifest.chmod(0o444)
  lock.chmod(0o644)
  lock.write_bytes(lock_data+b'\n[[package]]\nname="foreign"\nversion="1.0.0"\nsource="registry+https://example.invalid/index"\nchecksum="'+b'a'*64+b'"\n')
  lock.chmod(0o444)
  try: rejected('dependency source unsupported')
  finally:
   lock.chmod(0o644)
   lock.write_bytes(lock_data)
   lock.chmod(0o444)
  manifest=source/'Cargo.toml'
  manifest_data=manifest.read_bytes()
  manifest.chmod(0o644)
  manifest.write_bytes(manifest_data+b'\n[dependencies]\nescape={path="../../outside"}\n')
  manifest.chmod(0o444)
  try: rejected('source path unsupported')
  finally:
   manifest.chmod(0o644)
   manifest.write_bytes(manifest_data)
   manifest.chmod(0o444)
  manifest.chmod(0o644)
  manifest.write_bytes(manifest_data+b'\n[[bin]]\nname="escape"\npath="../../outside.rs"\n')
  manifest.chmod(0o444)
  try: rejected('source path unsupported')
  finally:
   manifest.chmod(0o644)
   manifest.write_bytes(manifest_data)
   manifest.chmod(0o444)
  path=source/'src/lib.rs'
  path.chmod(0o644)
  path.write_bytes(b'pub fn fixture() -> u32 { 99 }\n')
  path.chmod(0o444)
  try:
   try:
    executor._source_operation(manager,lifecycle,str(directory),context,'metadata',None,end)
    raise AssertionError('modified tracked source accepted')
   except executor.SourceExecutorError as error:
    assert str(error)=='source inventory mismatch'
  finally:
   path.chmod(0o644)
   path.write_bytes(original)
   path.chmod(0o444)
  source.chmod(0o755)
  extra=source/'untracked.txt'
  extra.write_text('unapproved')
  extra.chmod(0o444)
  source.chmod(0o555)
  try:
   try:
    executor._source_operation(manager,lifecycle,str(directory),context,'metadata',None,end)
    raise AssertionError('untracked source accepted')
   except executor.SourceExecutorError as error:
    assert str(error)=='source inventory mismatch'
  finally:
   source.chmod(0o755)
   extra.unlink()
   source.chmod(0o555)
  wrong_sha='0'*40 if commit!='0'*40 else '1'*40
  wrong_head=seal_context(dict(value,source_sha=wrong_sha))
  try:
   executor._source_operation(manager,lifecycle,str(directory),wrong_head,'metadata',None,end)
   raise AssertionError('wrong source head accepted')
  except executor.SourceExecutorError as error:
   assert str(error)=='source head mismatch'
  bad=dict(value)
  bad['tools']=dict(tools,cargo='0'*64)
  bad_fd=os.memfd_create('tampered-context',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
  try:
   os.write(bad_fd,json.dumps(bad,separators=(',',':')).encode())
   fcntl.fcntl(bad_fd,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL)
   bad_context=executor.read_context(bad_fd)
   try:
    executor._source_operation(manager,lifecycle,str(directory),bad_context,'metadata',None,end)
    raise AssertionError('wrong tool digest accepted')
   except executor.SourceExecutorError: pass
  finally: os.close(bad_fd)
  path=source/'src/lib.rs'
  path.chmod(0o666)
  try:
   executor._source_operation(manager,lifecycle,str(directory),context,'metadata',None,end)
   raise AssertionError('writable source accepted')
  except executor.SourceExecutorError: pass
  finally: path.chmod(0o444)
  assert (source/'src/lib.rs').read_bytes()==original
  assert snapshot()==before
  budget=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+32_000_000_000
  bounded=executor.SourceSession(manager,lifecycle,str(directory),initial,budget,(directory.stat().st_dev,directory.stat().st_ino))
  bounded.metadata()
  bounded.bind(context)
  remaining=(budget-25_100_000_000-time.clock_gettime_ns(time.CLOCK_BOOTTIME))/1_000_000_000
  if remaining>0: time.sleep(remaining)
  try:
   bounded.package(0,True)
   raise AssertionError('source session deadline was renewed')
  except lifecycle.SupervisorError: pass
  tcp_listener.settimeout(.5)
  try:
   duplicate,_=tcp_listener.accept()
   duplicate.close()
   raise AssertionError('package started after shared work budget expired')
  except TimeoutError: pass
  print('real fixed metadata and package operations passed; isolated Cargo config, sealed context and result, read-only synthetic source',flush=True)
 finally:
  manager.close()
  listener.close()
  tcp_listener.close()
  os.close(context_fd)
  watch.close()
