// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish};

const TOKEN: &str = "checkout-token-value";
pub(crate) const WAIT: Duration = Duration::from_secs(30);

pub(crate) struct TlsSink {
    server: Child,
    _stdin: ChildStdin,
    pub(crate) port: u16,
    request: Receiver<String>,
}

impl TlsSink {
    pub(crate) fn start(dir: &Path) -> Result<Self> {
        let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
        let made = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            ])
            .args(["-subj", "/CN=127.0.0.1", "-keyout"])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .output()
            .context("run openssl")?;
        if !made.status.success() {
            bail!("openssl req: {}", String::from_utf8_lossy(&made.stderr));
        }
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let mut server = Command::new("openssl")
            .args(["s_server", "-quiet", "-accept", &port.to_string()])
            .arg("-cert")
            .arg(&cert)
            .arg("-key")
            .arg(&key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("start openssl s_server")?;
        let stdin = server.stdin.take().context("s_server stdin")?;
        let stdout = server.stdout.take().context("s_server stdout")?;
        let (sender, request) = channel();
        std::thread::spawn(move || {
            let mut seen = String::new();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let end = line.trim().is_empty();
                seen.push_str(&line);
                seen.push('\n');
                if end {
                    break;
                }
            }
            let _ = sender.send(seen);
        });
        let sink = Self {
            server,
            _stdin: stdin,
            port,
            request,
        };
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Ok(sink);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("openssl s_server did not listen on {port}")
    }

    pub(crate) fn url(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    pub(crate) fn first_request(&self) -> Result<String> {
        self.request
            .recv_timeout(WAIT)
            .context("no request reached the TLS endpoint")
    }
}

impl TlsSink {
    pub(crate) fn run_unvisited(&self, mut runner: Child) -> Result<Ran> {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Ok(request) = self.request.try_recv() {
                let _ = runner.kill();
                let _ = runner.wait();
                bail!("the credential reached an unintended endpoint:\n{request}");
            }
            if runner.try_wait()?.is_some() {
                return finish(runner);
            }
            if Instant::now() > deadline {
                let _ = runner.kill();
                let _ = runner.wait();
                bail!("velnor-runner did not exit");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for TlsSink {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn trace(rig: &Rig) -> String {
    std::fs::read_to_string(rig.trace()).unwrap_or_default()
}

#[test]
fn token_reaches_https_origin_as_scoped_header_and_never_argv() -> Result<()> {
    let rig = Rig::new()?;
    let sink = TlsSink::start(&rig.root)?;
    let job = rig.job("checkout_depth1", &rig.head, &sink.url())?;
    let scratch = Scratch::new()?;
    crate::native_checkout_env::wrapper_env(&rig.root, "export GIT_SSL_NO_VERIFY=1\n")?;
    let runner = rig.start(&job, &scratch)?;
    let request = sink.first_request()?;
    drop(sink);
    let ran = finish(runner)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    let credential = STANDARD.encode(format!("x-access-token:{TOKEN}"));
    assert!(
        request.starts_with("GET /tailrocks/velnor-new.git/info/refs?"),
        "{request}"
    );
    assert_eq!(
        header(&request, "authorization"),
        Some(format!("basic {credential}").as_str()),
        "{request}"
    );
    let argv = trace(&rig);
    assert!(argv.contains("\"cmd_name\""), "git trace is empty");
    for text in [&argv, &ran.stdout, &ran.stderr] {
        assert!(
            !text.contains(TOKEN) && !text.contains(&credential),
            "the credential leaked: {text}"
        );
    }
    Ok(())
}

#[test]
fn token_is_not_sent_over_plain_http() -> Result<()> {
    let rig = Rig::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let (sender, request) = channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut seen = Vec::new();
        let mut byte = [0_u8; 1];
        while !seen.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
            seen.push(byte[0]);
        }
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        let _ = sender.send(String::from_utf8_lossy(&seen).into_owned());
    });
    let job = rig.job("checkout_depth1", &rig.head, &url)?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    let request = request.recv_timeout(WAIT).context("no request arrived")?;
    assert!(
        request.starts_with("GET /tailrocks/velnor-new.git/info/refs?"),
        "{request}"
    );
    assert_eq!(header(&request, "authorization"), None, "{request}");
    Ok(())
}

#[test]
fn cancel_during_the_fetch_stops_the_step_and_frees_the_mirror() -> Result<()> {
    let rig = Rig::new()?;
    let sink = TlsSink::start(&rig.root)?;
    let job = rig.job("checkout_depth1", &rig.head, &sink.url())?;
    let scratch = Scratch::new()?;
    crate::native_checkout_env::wrapper_env(&rig.root, "export GIT_SSL_NO_VERIFY=1\n")?;
    let mut runner = rig.start(&job, &scratch)?;
    sink.first_request()?;
    let signalled = Command::new("kill")
        .args(["-TERM", &runner.id().to_string()])
        .status()
        .context("run kill")?;
    assert!(signalled.success());
    let deadline = Instant::now() + WAIT;
    while runner.try_wait()?.is_none() {
        if Instant::now() > deadline {
            let _ = runner.kill();
            let _ = runner.wait();
            bail!("the fetch was not cancelled");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let ran = finish(runner)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(ran.stdout.contains("cancelled"), "{}", ran.stdout);
    assert!(!scratch.workspace().join(".git").exists());
    drop(sink);
    let again = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(again.code, Some(0), "{}\n{}", again.stdout, again.stderr);
    Ok(())
}
