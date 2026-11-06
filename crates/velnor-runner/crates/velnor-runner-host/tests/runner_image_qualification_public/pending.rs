// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::io::Read;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const CURL: &str = r#"#!/usr/bin/python3
import json, os, re, sys, time
s=sys.stdin.read()
u=re.search(r'url = "([^"]+)"',s).group(1)
o=re.search(r'output = "([^"]+)"',s).group(1)
m=re.search(r'request = "([^"]+)"',s).group(1)
root=os.environ['FIXTURE_DIR']
with open(root+'/requests','a') as f: f.write(m+' '+u+'\n')
status=200
body={}
if 'registration-token' in u:
 status=201; body={'token':'fixture-registration'}
elif 'runner-registration' in u:
 body={'url':'https://pipelines.actions.githubusercontent.com','token':'fixture-admin'}
elif '/sessions' in u and m=='POST':
 body={'sessionId':'fixture-session','messageQueueUrl':'https://pipelines.actions.githubusercontent.com/queue','messageQueueAccessToken':'fixture-queue'}
elif '/queue' in u and m=='GET':
 deadline=time.monotonic()+15
 while not os.path.exists(root+'/proxy-accepted') and time.monotonic()<deadline: time.sleep(.02)
 if not os.path.exists(root+'/delivered'):
  open(root+'/delivered','w').close()
  body={'messageId':7,'messageType':'RunnerScaleSetJobMessages','body':json.dumps([{'messageType':'JobCompleted','runnerRequestId':11,'runnerId':12,'runnerName':'fixture-worker'}])}
 else: status=202; body={}
elif '/queue/7' in u and m=='DELETE':
 open(root+'/acked','w').close(); status=204
elif '/runnerscalesets' in u and m=='GET':
 body={'count':1,'value':[{'id':1,'name':'ubuntu-26.04-scale-set','labels':[{'name':'velnor','type':'System'},{'name':'ubuntu-26.04-scale-set','type':'System'}],'runnerSetting':{'disableUpdate':True}}]}
elif '/sessions' in u and m=='DELETE': status=204
else: status=404
with open(o,'w') as f: json.dump(body,f)
print(status)
"#;

fn checked(command: &mut Command) -> TestResult<std::process::Output> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(output)
}

fn build_example(
    root: &std::path::Path,
    target: &std::path::Path,
) -> TestResult<std::path::PathBuf> {
    let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = original
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace")?;
    let destination = root.join("crates/velnor-runner");
    fs::create_dir_all(&destination)?;
    for name in ["Cargo.toml", "Cargo.lock", "crates"] {
        checked(
            Command::new("cp")
                .arg("-R")
                .arg(workspace.join(name))
                .arg(&destination),
        )?;
    }
    fs::create_dir(root.join(".velnor"))?;
    fs::copy(
        workspace.join("../../.velnor/runner.lock"),
        root.join(".velnor/runner.lock"),
    )?;
    let example = destination.join("crates/velnor-runner-host/examples/pending_fixture.rs");
    fs::write(example, include_str!("pending_example.txt"))?;
    checked(Command::new("/usr/bin/git").args(["init", "-q"]).arg(root))?;
    checked(
        Command::new("/usr/bin/git")
            .arg("-C")
            .arg(root)
            .args(["add", "."]),
    )?;
    checked(Command::new("/usr/bin/git").arg("-C").arg(root).args([
        "-c",
        "user.name=Alexey Zhokhov",
        "-c",
        "user.email=alexey@zhokhov.com",
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "-s",
        "-q",
        "-m",
        "fixture",
        "-m",
        "Co-authored-by: Codex <codex@openai.com>",
    ]))?;
    let source = checked(
        Command::new("/usr/bin/git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"]),
    )?;
    let source = String::from_utf8(source.stdout)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    checked(
        Command::new(cargo)
            .args(["build", "--offline", "--locked", "--manifest-path"])
            .arg(destination.join("Cargo.toml"))
            .args(["-p", "velnor-runner-host", "--example", "pending_fixture"])
            .env("CARGO_TARGET_DIR", target)
            .env("VELNOR_RUNNER_BUILD_SOURCE_SHA", source.trim())
            .env("VELNOR_RUNNER_BUILD_HELPER_SHA256", "a".repeat(64)),
    )?;
    Ok(target.join("debug/examples/pending_fixture"))
}

type Proxy = (
    std::net::SocketAddr,
    Arc<AtomicBool>,
    Arc<AtomicBool>,
    std::thread::JoinHandle<()>,
);

fn stalled_proxy(root: &std::path::Path) -> TestResult<Proxy> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let disconnected = Arc::new(AtomicBool::new(false));
    let socket_closed = Arc::clone(&disconnected);
    let marker = root.join("proxy-accepted");
    let proxy = std::thread::spawn(move || {
        while !stopped.load(Ordering::Acquire) {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut bytes = [0; 4096];
                drop(stream.read(&mut bytes));
                drop(fs::write(&marker, b"accepted"));
                drop(stream.set_nonblocking(true));
                while !stopped.load(Ordering::Acquire) {
                    match stream.peek(&mut bytes) {
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Ok(0) | Err(_) => {
                            socket_closed.store(true, Ordering::Release);
                            break;
                        }
                        Ok(_) => {}
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    Ok((address, stop, disconnected, proxy))
}

#[tokio::test]
async fn public_completion_is_acknowledged_during_pending_image_bootstrap() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let executable = build_example(root, &root.join("target"))?;
    let api = DockerApiFixture::new(ApiPlan::new(&"a".repeat(40)))?;
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let curl = bin.join("curl");
    fs::write(&curl, CURL)?;
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755))?;
    let (address, stop, disconnected, proxy) = stalled_proxy(root)?;
    let journal_path = root.join("journal.db");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
    let mut child = Command::new(executable)
        .env("HOME", root)
        .env("PATH", path)
        .env("FIXTURE_DIR", root)
        .env("FIXTURE_SOCKET", api.socket())
        .env("FIXTURE_JOURNAL", &journal_path)
        .env("HTTPS_PROXY", format!("http://{address}"))
        .env("https_proxy", format!("http://{address}"))
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(25);
    while !root.join("acked").exists() && Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let acked = root.join("acked").exists();
    let proxy_pending =
        root.join("proxy-accepted").exists() && !disconnected.load(Ordering::Acquire);
    drop(child.kill());
    let output = child.wait_with_output()?;
    stop.store(true, Ordering::Release);
    proxy.join().map_err(|_| "proxy thread")?;
    assert!(
        acked && proxy_pending,
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        fs::read_to_string(root.join("requests")).unwrap_or_default()
    );
    let database = turso::Builder::new_local(journal_path.to_str().ok_or("journal")?)
        .build()
        .await?;
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT raw_body FROM completion_inbox WHERE message_id = 7",
            (),
        )
        .await?;
    assert!(
        rows.next().await?.is_some(),
        "completion was not durably recorded"
    );
    let requests = fs::read_to_string(root.join("requests"))?;
    assert!(
        !requests.contains("jitconfig"),
        "minted a worker during pending bootstrap"
    );
    assert!(
        api.snapshot()?.containers.is_empty(),
        "started a container during pending bootstrap"
    );
    Ok(())
}
