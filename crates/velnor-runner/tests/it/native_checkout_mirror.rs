// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish, git};

const TOKEN: &str = "checkout-token-value";
const WAIT: Duration = Duration::from_secs(30);

fn plant_hook(dir: &Path, name: &str, marker: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let hook = dir.join(name);
    std::fs::write(
        &hook,
        format!("#!/bin/sh\necho ran > '{}'\n", marker.display()),
    )?;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn warm_mirror_then_advance(rig: &Rig) -> Result<String> {
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    rig.advance("next")
}

#[test]
fn planted_mirror_hook_does_not_run() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let marker = rig.root.join("hook-ran");
    plant_hook(
        &rig.mirror().join("hooks"),
        "reference-transaction",
        &marker,
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(!marker.exists(), "the mirror's hook ran");
    Ok(())
}

#[test]
fn planted_mirror_config_is_replaced_by_the_template() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let marker = rig.root.join("hook-ran");
    let evil_hooks = rig.root.join("evil-hooks");
    plant_hook(&evil_hooks, "reference-transaction", &marker)?;
    let config = rig.mirror().join("config");
    let mut planted = std::fs::OpenOptions::new().append(true).open(&config)?;
    write!(
        planted,
        "[core]\n\thooksPath = {}\n[url \"file:///nowhere/\"]\n\tinsteadOf = file://\n",
        evil_hooks.display()
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(!marker.exists(), "a hook from the planted config ran");
    let after = std::fs::read_to_string(&config)?;
    assert!(
        !after.contains("insteadOf") && !after.contains("hooksPath"),
        "{after}"
    );
    Ok(())
}

#[test]
fn host_git_config_does_not_apply() -> Result<()> {
    let rig = Rig::new()?;
    let global = rig.root.join("global-gitconfig");
    std::fs::write(
        &global,
        "[url \"file:///nowhere/\"]\n\tinsteadOf = file://\n",
    )?;
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let scratch = Scratch::new()?;
    let env = [("GIT_CONFIG_GLOBAL", global.to_str().context("utf-8")?)];
    let ran = finish(rig.start_env(&job, &scratch, &env)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    Ok(())
}

#[test]
fn stale_ref_locks_are_removed_before_the_fetch() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let heads = rig.mirror().join("refs/heads");
    let locks = [heads.join("main.lock"), heads.join("nested/other.lock")];
    std::fs::create_dir_all(heads.join("nested"))?;
    for lock in &locks {
        std::fs::write(lock, "")?;
    }
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?, next);
    assert!(locks.iter().all(|lock| !lock.exists()));
    Ok(())
}

#[test]
fn failed_fetch_that_stored_the_commit_still_succeeds() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    std::fs::write(rig.mirror().join("refs/heads/main"), "not a commit id\n")?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    Ok(())
}

#[test]
fn mirror_is_keyed_by_host_owner_and_repository() -> Result<()> {
    let rig = Rig::new()?;
    let closed = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let elsewhere = format!("http://127.0.0.1:{closed}");
    let other_owner = [("tailrocks/velnor-new", "someone/velnor-new")];
    let runs = [
        rig.job("checkout_depth1", &rig.head, &rig.server_url())?,
        rig.job("checkout_depth1", &rig.head, &elsewhere)?,
        rig.job_edit(
            "checkout_depth1",
            &rig.head,
            &rig.server_url(),
            &other_owner,
        )?,
    ];
    let codes = runs
        .iter()
        .map(|job| {
            let scratch = Scratch::new()?;
            Ok(finish(rig.start(job, &scratch)?)?.code)
        })
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(codes, [Some(0), Some(1), Some(1)]);
    let git_dir = rig.store().join("git");
    for mirror in [
        "_local/tailrocks/velnor-new.git",
        &format!("127.0.0.1+{closed}/tailrocks/velnor-new.git"),
        "_local/someone/velnor-new.git",
    ] {
        assert!(git_dir.join(mirror).join("HEAD").is_file(), "{mirror}");
    }
    Ok(())
}

#[test]
fn persist_credentials_accepts_only_upstream_spellings() -> Result<()> {
    let rig = Rig::new()?;
    for (spelling, accepted) in [("False", true), ("FALSE", true), ("fAlse", false)] {
        let job = rig.job_edit(
            "checkout_depth1",
            &rig.head,
            &rig.server_url(),
            &[("\"lit\": \"false\"", &format!("\"lit\": \"{spelling}\""))],
        )?;
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        let expected = if accepted { Some(0) } else { Some(1) };
        assert_eq!(
            ran.code, expected,
            "{spelling}: {}\n{}",
            ran.stdout, ran.stderr
        );
    }
    Ok(())
}

struct TlsSink {
    server: Child,
    _stdin: ChildStdin,
    port: u16,
    request: Receiver<String>,
}

impl TlsSink {
    fn start(dir: &Path) -> Result<Self> {
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

    fn url(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    fn first_request(&self) -> Result<String> {
        self.request
            .recv_timeout(WAIT)
            .context("no request reached the TLS endpoint")
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
    let env = [("GIT_SSL_NO_VERIFY", "1")];
    let runner = rig.start_env(&job, &scratch, &env)?;
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
