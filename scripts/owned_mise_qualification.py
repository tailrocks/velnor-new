#!/usr/bin/env python3
"""Native owned-Mise qualification; isolated fixtures, no repository mutations."""
import argparse, hashlib, http.server, json, os, platform, shutil, subprocess, tarfile, tempfile, threading
from pathlib import Path
from owned_tool_behavior import valid_mise_cases

ap=argparse.ArgumentParser()
ap.add_argument("--mise", required=True)
ap.add_argument("--source-commit", required=True)
ap.add_argument("--upstream-commit", required=True)
ap.add_argument("--expected-version", required=True)
ap.add_argument("--source-diff-sha256")
ap.add_argument("--output", required=True)
a=ap.parse_args()
binary=Path(a.mise).resolve(strict=True)
root=Path(tempfile.mkdtemp(prefix="velnor-owned-mise-qualification-")).resolve()
results=[]

def write(path,text,mode=0o600):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(text)
    path.chmod(mode)
    return path

def run(label,argv,env,cwd,want=0,expected=None):
    proc=subprocess.run([str(binary),*argv],env=env,cwd=cwd,text=True,capture_output=True,timeout=40)
    sentinel=(root/"sentinel").exists()
    owner_executed=(root/"owner-executed").exists()
    ok=(proc.returncode==0 if want==0 else proc.returncode!=0) and not sentinel
    if want != 0: ok=ok and not owner_executed
    if expected is not None: ok=ok and expected in proc.stdout
    results.append(dict(case=label,argv=argv,cwd=str(cwd),returncode=proc.returncode,stdout=proc.stdout,stderr=proc.stderr,sentinel=sentinel,owner_executed=owner_executed,passed=ok))
    if sentinel: (root/"sentinel").unlink()
    if owner_executed: (root/"owner-executed").unlink()
    return proc

env={key:os.environ[key] for key in ("PATH","SYSTEMROOT","COMSPEC","SSL_CERT_FILE","SSL_CERT_DIR","LANG","LC_ALL") if key in os.environ}
for key,sub in [("HOME","home"),("MISE_CONFIG_DIR","config"),("MISE_DATA_DIR","data"),("MISE_CACHE_DIR","cache"),("MISE_STATE_DIR","state"),("MISE_SYSTEM_CONFIG_DIR","system"),("MISE_RUSTUP_HOME","rustup"),("RUSTUP_HOME","rustup"),("MISE_CARGO_HOME","cargo"),("CARGO_HOME","cargo")]:
    env[key]=str(root/sub); (root/sub).mkdir(parents=True,exist_ok=True)
env.update(MISE_NO_ENV="1",MISE_NO_HOOKS="1",MISE_YES="1",MISE_AUTO_INSTALL="0",MISE_COLOR="0",MISE_PARANOID="1")
ambient=root/"ambient"
write(ambient/"cargo",f'#!/bin/sh\ntouch "{root}/sentinel"\nprintf "AMBIENT_FALLBACK_EXECUTED\\n"\nexit 86\n',0o700)
write(ambient/"mbx",(ambient/"cargo").read_text(),0o700)
env["PATH"]=str(ambient)+os.pathsep+env.get("PATH","")
work=root/"work"; nested=work/"nested"; nested.mkdir(parents=True)
poison="fixture invalid toml [\n"
configs=[work/".miserc.toml",nested/".miserc.toml",work/"mise.toml",nested/"mise.toml",root/"home/.miserc.toml",root/"home/.config/mise/config.toml",root/"config/config.toml",root/"system/config.toml"]
configs += [work/".miserc.local.toml",nested/".miserc.local.toml",work/".config/miserc.toml",nested/".config/miserc.toml",root/"config/miserc.toml",root/"config/miserc.local.toml",root/"system/miserc.toml"]
for p in configs: write(p,poison)
# NoConfig must apply before settings/config parsing, including help/version.
for cwd in (work,nested):
    run("cli-no-config-version-"+cwd.name,["--no-config","version"],env,cwd,expected=a.expected_version)
    run("env-no-config-version-"+cwd.name,["--version"],dict(env,MISE_NO_CONFIG="1"),cwd,expected=a.expected_version)
    run("cli-no-config-exec-"+cwd.name,["--no-config","exec","--","/usr/bin/true"],env,cwd)
    run("env-no-config-exec-"+cwd.name,["exec","--","/usr/bin/true"],dict(env,MISE_NO_CONFIG="1"),cwd)
# Build a fake pinned Rust installation. Test native Mise dispatch, not Rust compilation.
realbin=root/"cargo/bin"
write(realbin/"cargo",'#!/bin/sh\nprintf "REAL_CARGO:%s:%s\\n" "$MBX_CARGO_SHIM_MODE" "$*"\nif [ "$1" = "fidelity" ]; then shift; pwd; printf "ARG:%s\\n" "$@"; exit 37; fi\n',0o700)
write(realbin/"rustc",'#!/bin/sh\nprintf "rustc 1.99.0 (fixture)\\n"\n',0o700)
write(realbin/"rustup",'#!/bin/sh\nexit 0\n',0o700)
install=root/"data/installs/rust/1.99.0"; install.parent.mkdir(parents=True,exist_ok=True); install.symlink_to(realbin)
owner=write(root/"owner/bin/mbx",f'#!/bin/sh\ntouch "{root}/owner-executed"\nprintf "OWNED_MBX:%s\\n" "$MBX_CARGO_SHIM_MODE"\nexec cargo "$@"\n',0o700)
env.update(MISE_NO_CONFIG="1",MISE_OWNED_CARGO_WRAPPER=str(owner),MISE_OWNED_CARGO_WRAPPER_SHA256=hashlib.sha256(owner.read_bytes()).hexdigest())
exclusive=["exec","rust@1.99.0","--","cargo","check"]
for cwd in (work,nested): run("exclusive-owned-"+cwd.name,exclusive,env,cwd,expected="OWNED_MBX:1\nREAL_CARGO:1:check")
write(ambient/"mise",(ambient/"cargo").read_text(),0o700)
run("poison-ambient-mise-authority",exclusive,dict(env,MISE_BIN=str(ambient/"mise")),nested,expected="OWNED_MBX:1\nREAL_CARGO:1:check")
fidelity=run("native-cwd-argv-exit",["exec","rust@1.99.0","--","cargo","fidelity","has spaces","quote\"literal","Unicode-λ","--flag=literal"],env,nested,want=37)
# Expected owner execution and exact nonzero exit belong to this positive fidelity case.
results[-1]["passed"]=fidelity.returncode==37 and str(nested.resolve())+"\nARG:has spaces\nARG:quote\"literal\nARG:Unicode-λ\nARG:--flag=literal\n" in fidelity.stdout and not results[-1]["sentinel"]
mutations={"relative-owner":{"MISE_OWNED_CARGO_WRAPPER":"owner/bin/mbx"},"nonliteral-owner":{"MISE_OWNED_CARGO_WRAPPER":"{{env.HOME}}/mbx"},"injection-owner":{"MISE_OWNED_CARGO_WRAPPER":str(owner)+"; touch sentinel"},"missing-owner":{"MISE_OWNED_CARGO_WRAPPER":str(root/"missing/mbx")},"wrong-digest":{"MISE_OWNED_CARGO_WRAPPER_SHA256":"0"*64},"uppercase-digest":{"MISE_OWNED_CARGO_WRAPPER_SHA256":env["MISE_OWNED_CARGO_WRAPPER_SHA256"].upper()},"malformed-digest":{"MISE_OWNED_CARGO_WRAPPER_SHA256":"g"*64},"empty-digest":{"MISE_OWNED_CARGO_WRAPPER_SHA256":""},"empty-owner":{"MISE_OWNED_CARGO_WRAPPER":""},"no-config-disabled":{"MISE_NO_CONFIG":"0"},"no-env-disabled":{"MISE_NO_ENV":"0"},"no-hooks-disabled":{"MISE_NO_HOOKS":"0"}}
for name,changes in mutations.items(): run(name,exclusive,dict(env,**changes),nested,want=1)
for field in ("MISE_OWNED_CARGO_WRAPPER","MISE_OWNED_CARGO_WRAPPER_SHA256"):
    mutated=env.copy();del mutated[field];run("missing-"+field,exclusive,mutated,nested,want=1)
for flag in ("MISE_NO_CONFIG","MISE_NO_ENV","MISE_NO_HOOKS"):
    missing=env.copy();del missing[flag];run("missing-"+flag,exclusive,missing,nested,want=1)
    run("nonexact-true-"+flag,exclusive,dict(env,**{flag:"true"}),nested,want=1)
owner.chmod(0o600);run("owner-no-execute",exclusive,env,nested,want=1);owner.chmod(0o755)
run("owner-writable-0755-accepted",exclusive,env,nested,expected="OWNED_MBX:1\nREAL_CARGO:1:check");owner.chmod(0o700)
run("owner-noncanonical-parent-path",exclusive,dict(env,MISE_OWNED_CARGO_WRAPPER=str(owner.parent)+"/../bin/mbx"),nested,want=1)
# Cache shape belongs to the source wrapper, including exclusive directory inventory.
shim=root/"data/command-wrappers/bin/cargo"
shim.unlink();write(shim,f'#!/bin/sh\ntouch "{root}/sentinel"\nexit 86\n',0o700);run("cached-regular-shim-forged",exclusive,env,nested,want=1)
shim.unlink();shim.symlink_to(ambient/"cargo");run("cached-symlink-shim-forged",exclusive,env,nested,want=1)
foreign=root/"foreign-mise";shutil.copy2(binary,foreign);shim.unlink();shim.symlink_to(foreign);run("cached-foreign-mise-shim",exclusive,env,nested,want=1)
shim.unlink();run("missing-shim-native-provision",exclusive,env,nested,expected="OWNED_MBX:1\nREAL_CARGO:1:check")
results.append(dict(case="new-shim-exact-current-executable",passed=shim.is_symlink() and shim.readlink()==binary))
for name in ("rustc","mbx","cargo-nextest","other"):
    stale=write(shim.parent/name,(ambient/"cargo").read_text(),0o700);run("stale-wrapper-dir-"+name,exclusive,env,nested,want=1);stale.unlink()
shim.unlink();shim.symlink_to(owner);run("new-managed-shim-mutated-owner-target",exclusive,env,nested,want=1)
shim.unlink();run("native-reprovision",exclusive,env,nested,expected="OWNED_MBX:1\nREAL_CARGO:1:check")
wrapper_dir=shim.parent;renamed=wrapper_dir.with_name("bin-real");wrapper_dir.rename(renamed);wrapper_dir.symlink_to(renamed)
run("symlink-wrapper-directory",exclusive,env,nested,want=1);wrapper_dir.unlink();renamed.rename(wrapper_dir)
owner_target=owner.with_name("mbx-target");owner.rename(owner_target);owner.symlink_to(owner_target)
run("symlink-owner",exclusive,env,nested,want=1)
owner.unlink();owner_target.rename(owner)
owner.chmod(0o777);run("world-writable-owner",exclusive,env,nested,want=1);owner.chmod(0o700)
owner.chmod(0o720);run("group-writable-owner",exclusive,env,nested,want=1);owner.chmod(0o700)
original_owner=owner.read_bytes();owner.write_bytes(original_owner+b"# tampered\n");run("tampered-owner",exclusive,env,nested,want=1);owner.write_bytes(original_owner)
# Exact native HTTP backend ownership: one artifact GET, reinstall uses install state.
archive=root/"owner.tar.gz"
with tarfile.open(archive,"w:gz") as bundle: bundle.add(owner,arcname="bin/mbx")
archive_hash=hashlib.sha256(archive.read_bytes()).hexdigest()
requests=[]
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        requests.append(self.path)
        if self.path!="/owner.tar.gz": self.send_error(404);return
        body=archive.read_bytes();self.send_response(200);self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args): pass
server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Handler)
thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
url=f"http://127.0.0.1:{server.server_port}/owner.tar.gz"
spec=f"http:velnor-owned-mbx[url={url},checksum=sha256:{archive_hash},bin_path=bin,strip_components=0]@0.1.0"
install_env={k:v for k,v in env.items() if not k.startswith("MISE_OWNED_CARGO_WRAPPER")}
run("native-http-install",["install",spec],install_env,nested)
first_requests=len(requests)
run("native-http-reinstall",["install",spec],install_env,nested)
results.append(dict(case="native-http-single-install",requests=list(requests),passed=first_requests==1 and len(requests)==1))
bad_spec=f"http:velnor-bad-mbx[url={url},checksum=sha256:{'0'*64},bin_path=bin,strip_components=0]@0.1.0"
run("native-http-wrong-checksum",["install",bad_spec],install_env,nested,want=1)
server.shutdown();server.server_close()
installed=root/"data/installs/http-velnor-owned-mbx/0.1.0/bin/mbx"
if installed.exists():
    installed_env=dict(env,MISE_OWNED_CARGO_WRAPPER=str(installed),MISE_OWNED_CARGO_WRAPPER_SHA256=hashlib.sha256(installed.read_bytes()).hexdigest())
    run("native-http-owned-dispatch",exclusive,installed_env,nested,expected="OWNED_MBX:1\nREAL_CARGO:1:check")
    installed.write_bytes(installed.read_bytes()+b"# tampered before exec\n")
    run("native-http-tampered-beforeexec",exclusive,installed_env,nested,want=1)
else: results.append(dict(case="native-http-owned-dispatch",passed=False,error="missing expected native installation"))
version=subprocess.run([str(binary),"--version"],env=dict(env,MISE_NO_CONFIG="1"),cwd=root,text=True,capture_output=True)
report=dict(root=str(root),host=dict(system=platform.system(),machine=platform.machine()),source_commit=a.source_commit,source_diff_sha256=a.source_diff_sha256,upstream_commit=a.upstream_commit,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),version=version.stdout.strip(),version_is_distinct=version.stdout.strip().split()[0]==a.expected_version and a.expected_version!="2026.10.4",results=results)
Path(a.output).write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps({"output":a.output,"root":str(root),"passed":sum(r["passed"] for r in results),"total":len(results),"failed":[r["case"] for r in results if not r["passed"]]}))
raise SystemExit(0 if valid_mise_cases(results) and report["version_is_distinct"] else 1)
