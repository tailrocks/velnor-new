// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::release_plz_acquire::acquire_step;

#[test]
fn release_tool_acquisition_checks_download_and_archive_before_use()
-> Result<(), Box<dyn std::error::Error>> {
    let (runner, archive_hash, binary_hash) = if cfg!(target_os = "macos") {
        (
            "macos-15",
            "b2c6e636199c7fa9dae8838fbebb49cb99933908eb7dd67f25ec831bb38e1c52",
            "d725885957dfe31407a88a5eed28b7e402b47bd0abdff3f5655c896d7a03d723",
        )
    } else {
        (
            "ubuntu-26.04",
            "4a4cfa92ea2abaee02b40daa596a95ca2725816a6a32cb03a8f48ddead287681",
            "678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466",
        )
    };
    for (label, target, executable) in [
        (
            "ubuntu-26.04",
            "x86_64-unknown-linux-musl",
            "678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466",
        ),
        (
            "macos-15",
            "aarch64-apple-darwin",
            "d725885957dfe31407a88a5eed28b7e402b47bd0abdff3f5655c896d7a03d723",
        ),
    ] {
        let qualified = velnor_actions_mise::catalog::qualified_release_plz("0.4.3", label)
            .ok_or("unqualified tool")?;
        assert_eq!(qualified.target, target);
        assert_eq!(qualified.executable_sha256, executable);
        let StepKind::Shell { run, .. } = acquire_step("0.4.3", label)?.kind else {
            return Err("expected shell step".into());
        };
        assert!(run[2].contains(qualified.archive_sha256));
        assert!(run[2].contains(qualified.executable_sha256));
    }
    assert!(velnor_actions_mise::catalog::qualified_release_plz("0.4.4", runner).is_none());
    assert!(
        velnor_actions_mise::catalog::qualified_release_plz("0.4.3", "ubuntu-24.04-arm").is_none()
    );
    let step = acquire_step("0.4.3", runner)?;
    let StepKind::Shell { run, env } = step.kind else {
        return Err("expected shell step".into());
    };
    assert_eq!(
        env.get("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
            .map(String::as_str),
        Some("")
    );
    assert_eq!(
        env.get("ACTIONS_ID_TOKEN_REQUEST_URL").map(String::as_str),
        Some("")
    );
    assert!(run[2].contains(
        "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/"
    ));
    assert!(run[2].contains("--proto '=https'"));
    assert!(run[2].contains(archive_hash));
    assert!(run[2].contains(binary_hash));
    assert!(acquire_step("0.4.4", runner).is_err());
    assert!(acquire_step("0.4.3", "self-hosted").is_err());
    let temp = tempfile::tempdir()?;
    let harness = temp.path().join("harness.py");
    fs::write(&harness, HARNESS)?;
    let output = Command::new("python3")
        .arg(&harness)
        .arg(&run[2])
        .arg(archive_hash)
        .arg(binary_hash)
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

const HARNESS: &str = r#"
import hashlib, http.server, io, os, pathlib, re, socketserver, subprocess, sys, tarfile, tempfile, threading
script, original_archive, original_binary = sys.argv[1:]
binary=b'#!/bin/sh\ntouch "$RUNNER_TEMP/executed"\nexit 88\n'
def archive(entries):
    output=io.BytesIO()
    with tarfile.open(fileobj=output,mode='w:gz') as tar:
        for name, body in entries:
            info=tarfile.TarInfo(name)
            info.size=len(body)
            info.mode=0o755
            info.mtime=0
            tar.addfile(info,io.BytesIO(body))
    return output.getvalue()
good=archive([('release-plz',binary)])
class Handler(http.server.BaseHTTPRequestHandler):
    content=good
    status=200
    def log_message(self,*args): pass
    def do_GET(self):
        self.send_response(self.status)
        self.send_header('Content-Length',str(len(self.content)))
        self.end_headers()
        self.wfile.write(self.content)
class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address=True
    daemon_threads=True
with Server(('127.0.0.1',0),Handler) as server, tempfile.TemporaryDirectory() as root:
    threading.Thread(target=server.serve_forever,daemon=True).start()
    original_url=re.search(r"https://github.com/release-plz/release-plz/releases/download/[^']+",script).group(0)
    base=script.replace(original_url,f'http://127.0.0.1:{server.server_address[1]}/asset').replace("--proto '=https'","--proto '=http'")
    base=base.replace(original_binary,hashlib.sha256(binary).hexdigest())
    def run(name, body, trusted, status=200):
        Handler.content=body
        Handler.status=status
        tmp=pathlib.Path(root)/name
        tmp.mkdir()
        env={'PATH':'/usr/bin:/bin','HOME':root,'RUNNER_TEMP':str(tmp),'NO_PROXY':'*','no_proxy':'*','ACTIONS_ID_TOKEN_REQUEST_TOKEN':'test-sentinel','ACTIONS_ID_TOKEN_REQUEST_URL':'test-sentinel','GITHUB_TOKEN':'test-sentinel'}
        command=base.replace(original_archive,hashlib.sha256(trusted).hexdigest()).replace('--retry 5','--retry 0')
        unset='unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR;'
        assert unset in command,'credential scrub missing'
        command=command.replace(unset,unset+' /usr/bin/env > "$RUNNER_TEMP/observed-env";',1)
        result=subprocess.run(['/bin/sh','-c',command],env=env,capture_output=True,text=True,timeout=30)
        observed=(tmp/'observed-env').read_text()
        assert 'test-sentinel' not in observed,name+' leaked credentials into acquisition'
        assert not (tmp/'executed').exists(),name+' executed the binary before validation'
        return result,tmp
    result,tmp=run('good',good,good)
    assert result.returncode==0,('good',result.stderr)
    assert (tmp/'velnor-release-plz/release-plz').read_bytes()==binary
    result,tmp=run('incorrect-expected-digest',good,b'not the expected archive')
    assert result.returncode!=0 and not (tmp/'velnor-release-plz/release-plz').exists(),'incorrect expected digest'
    changed=bytearray(good)
    changed[4]^=1
    result,tmp=run('archive-digest',bytes(changed),good)
    assert result.returncode!=0 and not (tmp/'velnor-release-plz/release-plz').exists(),'archive digest'
    wrong=archive([('release-plz',b'wrong executable')])
    result,_=run('binary-digest',wrong,wrong)
    assert result.returncode!=0,'binary digest'
    duplicate=archive([('release-plz',binary),('release-plz',binary)])
    result,tmp=run('duplicate',duplicate,duplicate)
    assert result.returncode!=0 and not (tmp/'velnor-release-plz/release-plz').exists(),'duplicate entry'
    traversal=archive([('release-plz',binary),('../escape',b'bad')])
    result,tmp=run('traversal',traversal,traversal)
    assert result.returncode!=0 and not (tmp/'escape').exists(),'path traversal'
    result,_=run('http-error',good,good,404)
    assert result.returncode!=0,'HTTP failure'
    server.shutdown()
"#;

#[test]
fn semver_acquisition_rejects_modified_archive_and_executable()
-> Result<(), Box<dyn std::error::Error>> {
    let tool = velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-26.04")
        .ok_or("tool")?;
    assert!(velnor_actions_mise::catalog::qualified_cargo_semver_checks("self-hosted").is_none());
    let step =
        velnor_actions_workflow_renderer::release_plz_acquire::acquire_semver_step("ubuntu-26.04")?;
    let StepKind::Shell { run, env } = step.kind else {
        return Err("shell".into());
    };
    assert_eq!(
        env.get("CARGO_REGISTRY_TOKEN").map(String::as_str),
        Some("")
    );
    let dir = tempfile::tempdir()?;
    let harness = dir.path().join("harness.py");
    fs::write(&harness, SEMVER_HARNESS)?;
    let output = Command::new("python3")
        .arg(harness)
        .arg(&run[2])
        .arg(tool.archive_sha256)
        .arg(tool.executable_sha256)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

const SEMVER_HARNESS: &str = r#"
import hashlib,http.server,io,os,pathlib,re,socketserver,subprocess,sys,tarfile,tempfile,threading
script,archive_pin,binary_pin=sys.argv[1:]
if sys.platform == "darwin": script=script.replace("/usr/bin/sha256sum", "/usr/bin/shasum -a 256")
binary=b'#!/bin/sh\ntouch "$RUNNER_TEMP/executed"\nprintf "cargo-semver-checks 0.51.0\\n"\n'
def archive(content, name='cargo-semver-checks'):
    data=io.BytesIO()
    with tarfile.open(fileobj=data,mode='w:gz') as t:
        entry=tarfile.TarInfo(name); entry.size=len(content); entry.mode=0o755
        t.addfile(entry,io.BytesIO(content))
    return data.getvalue()
good=archive(binary)
class Handler(http.server.BaseHTTPRequestHandler):
    content=good
    def log_message(self,*args): pass
    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(self.content)
class Server(socketserver.ThreadingTCPServer): allow_reuse_address=True; daemon_threads=True
with Server(('127.0.0.1',0),Handler) as server, tempfile.TemporaryDirectory() as root:
    threading.Thread(target=server.serve_forever,daemon=True).start()
    url=re.search(r'https://github.com/obi1kenobi/[^\x27]+',script).group(0)
    base=script.replace(url,f'http://127.0.0.1:{server.server_address[1]}/asset').replace("--proto '=https'","--proto '=http'").replace('--retry 5','--retry 0').replace(binary_pin,hashlib.sha256(binary).hexdigest())
    def run(label,content,expected):
        Handler.content=content; temp=pathlib.Path(root)/label; temp.mkdir()
        command=base.replace(archive_pin,hashlib.sha256(expected).hexdigest())
        env={'PATH':'/usr/bin:/bin','HOME':root,'RUNNER_TEMP':str(temp),'NO_PROXY':'*'}
        result=subprocess.run(['/bin/sh','-c',command],env=env,capture_output=True,timeout=30)
        return result.returncode,(temp/'executed').exists()
    assert run('valid',good,good)==(0,True)
    assert run('bad_archive',good,b'incorrect')==(1,False)
    wrong=archive(b'bad executable')
    assert run('bad_binary',wrong,wrong)==(1,False)
    path=archive(binary,'../escape')
    assert run('bad_path',path,path)==(1,False)
    server.shutdown()
"#;
