// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::io::Repo;

type Requests = Arc<Mutex<Vec<String>>>;

pub(super) type TestResult = Result<(), Box<dyn Error>>;

struct Registry {
    url: String,
    requests: Requests,
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        drop(TcpStream::connect(self.address));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

pub(super) fn setup(root: &Path, registry: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["crates-mirror", "--registry", registry, "--cainfo"])
        .arg(root.join("ca.pem"))
        .env("CARGO_HOME", root.join("home"))
        .env("RUNNER_TEMP", root)
        .env("GITHUB_ENV", root.join("env"))
        .env("GITHUB_PATH", root.join("path"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn mirror_fixture(exercise: fn(&Repo, &Path, &Path) -> TestResult) -> TestResult {
    if rustup_binary().is_none() {
        eprintln!("SKIP crates_mirror integration fixture: no real rustup binary available");
        return Ok(());
    }
    let repo = Repo::empty("crates-mirror");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by the local HTTP fixture");
    repo.write_unstaged("shadow/cargo", "not executable");
    fs::set_permissions(root.join("shadow/cargo"), fs::Permissions::from_mode(0o644))?;
    repo.write_unstaged(
        "package/probe-1.0.0/Cargo.toml",
        "[package]\nname='probe'\nversion='1.0.0'\nedition='2024'\n",
    );
    repo.write_unstaged("package/probe-1.0.0/src/lib.rs", "");
    let packed = Command::new("tar")
        .env("COPYFILE_DISABLE", "1")
        .args(["-czf"])
        .arg(root.join("probe.crate"))
        .args(["-C"])
        .arg(root.join("package"))
        .arg("probe-1.0.0")
        .status()?;
    assert!(packed.success());
    let archive = fs::read(root.join("probe.crate"))?;
    let checksum = velnor_rust::sha256_hex(&archive);
    let registry = serve(archive, &checksum)?;
    for _ in 0..2 {
        let output = setup(root, &registry.url);
        assert!(output.status.success(), "{output:?}");
    }
    let config = root.join("home/config.toml");
    let text = fs::read_to_string(&config)?;
    assert!(text.contains(&root.join("ca.pem").display().to_string()));
    let env = fs::read_to_string(root.join("env"))?;
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    assert!(env.contains(&format!("CARGO={}", wrapper.display())));
    exercise(&repo, &wrapper, &config)?;
    let requests = registry
        .requests
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.as_str() == "/crates/probe/1.0.0/download")
            .count(),
        2,
        "{requests:?}"
    );
    Ok(())
}

fn serve(archive: Vec<u8>, checksum: &str) -> Result<Registry, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let registry = format!("sparse+http://{address}/index/");
    let responses = BTreeMap::from([
        ("/index/config.json".to_owned(), format!("{{\"dl\":\"http://{address}/crates\"}}").into_bytes()),
        ("/index/pr/ob/probe".to_owned(), format!("{{\"name\":\"probe\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"{checksum}\",\"features\":{{}},\"yanked\":false}}\n").into_bytes()),
        ("/crates/probe/1.0.0/download".to_owned(), archive),
    ]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let requests = seen.clone();
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = stopping.clone();
    let thread = std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                .unwrap_or_else(|e| unreachable!("{e}"));
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request);
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            requests
                .lock()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .push(path.to_owned());
            let (status, body) = responses
                .get(path)
                .map_or(("404 Not Found", &b""[..]), |body| {
                    ("200 OK", body.as_slice())
                });
            let header = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(header.as_bytes())
                .and_then(|()| stream.write_all(body))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    Ok(Registry {
        url: registry,
        requests: seen,
        address,
        stopping,
        thread: Some(thread),
    })
}

pub(super) fn active_toolchain() -> Result<String, Box<dyn Error>> {
    let output = Command::new(rustup_binary().ok_or("rustup unavailable")?)
        .args(["show", "active-toolchain"])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let active = String::from_utf8(output.stdout)?;
    let active = active.trim();
    let toolchain = active.rsplit_once(" (").map_or(active, |(name, _)| name);
    if toolchain.is_empty() {
        return Err("active toolchain".into());
    }
    Ok(toolchain.to_owned())
}

pub(super) fn rustup_binary() -> Option<&'static Path> {
    static RUSTUP: OnceLock<Option<PathBuf>> = OnceLock::new();
    RUSTUP
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
            let candidate = |path: PathBuf| {
                if path.components().any(|part| part.as_os_str() == "shims") {
                    return None;
                }
                let path = fs::canonicalize(path).ok()?;
                if path.file_stem()? != "rustup" {
                    return None;
                }
                let output = Command::new(&path)
                    .arg("--version")
                    .current_dir(&root)
                    .output()
                    .ok()?;
                (output.status.success() && output.stdout.starts_with(b"rustup ")).then_some(path)
            };
            if let Some(path) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .find_map(|directory| candidate(directory.join("rustup")))
            {
                return Some(path);
            }
            let output = Command::new("mise")
                .args(["which", "rustup"])
                .current_dir(&root)
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            candidate(PathBuf::from(String::from_utf8(output.stdout).ok()?.trim()))
        })
        .as_deref()
}
