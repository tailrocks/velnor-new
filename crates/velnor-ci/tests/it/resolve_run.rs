// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Runs one world through `velnor-ci resolve-qualification` against a local fake API, or through
//! the legacy engine against a fake `mise` that answers `gh api` from the same recorded answers.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::resolve_world::{Quirk, World, copy_repo, data_dir, sha256};
use crate::support::fresh_dir;

static LAST_MICROS: AtomicU64 = AtomicU64::new(0);

/// How long the last `command.output()` of a runner took.
pub(crate) fn last_micros() -> u64 {
    LAST_MICROS.load(Ordering::Relaxed)
}

fn timed(command: &mut Command) -> Output {
    let start = Instant::now();
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    LAST_MICROS.store(
        u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    output
}

const ADMISSION: &str = "velnor/qualification-cache-admission.json";

/// What a run did: whether it succeeded, the error it reported, the API routes it asked for in
/// order and the admission file it staged.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) error: String,
    pub(crate) requests: Vec<String>,
    pub(crate) admission: Option<Vec<u8>>,
}

fn error_of(output: &Output) -> String {
    if output.status.success() {
        return String::new();
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.trim_end().lines().last().unwrap_or("");
    match line.rsplit_once("internal request failed: ") {
        Some((_, code)) => code.strip_prefix("internal: ").unwrap_or(code).to_owned(),
        None => format!("RAW: {line}"),
    }
}

fn sanitize(route: &str) -> String {
    route
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Lay the world out under a fresh directory: the repository, `$RUNNER_TEMP` with the request,
/// the event payload.
struct Laid {
    root: PathBuf,
    repo: PathBuf,
    temp: PathBuf,
    payload: PathBuf,
    request: PathBuf,
}

fn lay(world: &World, request_name: &str) -> Laid {
    let root = fresh_dir("velnor-ci-resolve", "world");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    let temp = root.join("rt");
    let dir = temp.join("velnor/request");
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let name = world.request_name.as_deref().unwrap_or(request_name);
    let request = dir.join(name);
    let bytes = world
        .request_bytes
        .clone()
        .unwrap_or_else(|| world.request.to_string().into_bytes());
    if world.quirk == Some(Quirk::RequestIsASymlink) {
        fs::write(root.join("target.json"), &bytes).unwrap_or_else(|e| unreachable!("{e}"));
        std::os::unix::fs::symlink(root.join("target.json"), &request)
            .unwrap_or_else(|e| unreachable!("{e}"));
    } else if world.quirk == Some(Quirk::RequestIsAFifo) {
        crate::support::make_fifo(&request);
    } else {
        fs::write(&request, &bytes).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let payload = root.join("event.json");
    let text = world
        .payload_bytes
        .clone()
        .unwrap_or_else(|| world.payload.to_string().into_bytes());
    fs::write(&payload, text).unwrap_or_else(|e| unreachable!("{e}"));
    if world.quirk == Some(Quirk::AdmissionAlreadyStaged) {
        fs::write(temp.join(ADMISSION), b"x").unwrap_or_else(|e| unreachable!("{e}"));
    }
    if world.quirk == Some(Quirk::VelnorDirectoryIsASymlink) {
        let elsewhere = root.join("elsewhere");
        fs::rename(temp.join("velnor"), &elsewhere).unwrap_or_else(|e| unreachable!("{e}"));
        std::os::unix::fs::symlink(&elsewhere, temp.join("velnor"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    Laid {
        root,
        repo,
        temp,
        payload,
        request,
    }
}

fn admission_of(temp: &Path) -> Option<Vec<u8>> {
    fs::read(temp.join(ADMISSION))
        .ok()
        .filter(|_| !temp.join(ADMISSION).is_symlink())
}

fn context_env(world: &World, command: &mut Command) {
    for (key, value) in &world.env {
        command.env(key, value);
    }
}

type Log = Arc<Mutex<Vec<String>>>;

/// A local HTTP server answering GETs from the world's API answers; every path asked for is
/// logged. It answers 404 for a route the world does not hold.
fn serve(world: &World) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let answers = world.api.clone();
    let log: Log = Arc::default();
    let seen = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .trim_start_matches('/')
                .to_owned();
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(path.clone());
            let mut stream = &stream;
            let reply = match answers.get(&path) {
                Some(body) => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream
                        .write_all(head.as_bytes())
                        .and_then(|()| stream.write_all(body))
                }
                None => stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                ),
            };
            drop(reply);
        }
    });
    (format!("http://{address}"), log)
}

pub(crate) fn run_helper(world: &mut World, bin: &str) -> Outcome {
    world.finish();
    let laid = lay(world, "plan-v1-request.json");
    let (api, log) = serve(world);
    if world.quirk != Some(Quirk::NotARepository) {
        copy_repo(&laid.repo);
    }
    let mut command = Command::new(bin);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &laid.root)
        .current_dir(if world.quirk == Some(Quirk::NotARepository) {
            &laid.root
        } else {
            &laid.repo
        });
    command
        .env("RUNNER_TEMP", &laid.temp)
        .env("GH_TOKEN", "token")
        .env("GITHUB_API_URL", api)
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_EVENT_PATH", &laid.payload);
    context_env(world, &mut command);
    for name in &world.unset {
        command.env_remove(name);
    }
    command.arg("resolve-qualification");
    if world.request_name.is_some() {
        command.arg("--request").arg(&laid.request);
    }
    let output = timed(&mut command);
    let outcome = Outcome {
        error: error_of(&output),
        requests: log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        admission: admission_of(&laid.temp),
    };
    fs::remove_dir_all(&laid.root).unwrap_or_else(|e| unreachable!("{e}"));
    outcome
}

/// The legacy engine in the same world. The legacy preparation wants an installed release; the
/// manifest naming this binary stands in for it and is not committed to the repository.
pub(crate) fn run_legacy(world: &mut World, engine: &str) -> Outcome {
    world.finish();
    let laid = lay(world, "plan-request.json");
    if world.quirk != Some(Quirk::NotARepository) {
        copy_repo(&laid.repo);
    }
    let bin_dir = laid.root.join("bin");
    let api_dir = laid.root.join("api");
    fs::create_dir_all(&bin_dir).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&api_dir).unwrap_or_else(|e| unreachable!("{e}"));
    fs::copy(data_dir().join("fake-mise.sh"), bin_dir.join("mise"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for (route, body) in &world.api {
        fs::write(api_dir.join(sanitize(route)), body).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let engine_sha = sha256(&fs::read(engine).unwrap_or_else(|e| unreachable!("{e}")));
    let record = |target: &str| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v0.1.82/velnor-actions-0.1.82-{target}\",\"sha256\":\"{}\"}}",
            &engine_sha["sha256:".len()..]
        )
    };
    let manifest = format!(
        "{{\"schema\":1,\"version\":\"0.1.82\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"7d9340921fbe06bb23ca5f02c071c65870c39277\",\"targets\":[{},{},{}]}}\n",
        record("aarch64-apple-darwin"),
        record("x86_64-apple-darwin"),
        record("x86_64-unknown-linux-gnu")
    );
    if world.quirk != Some(Quirk::NotARepository) {
        fs::write(laid.repo.join(".velnor/release-manifest.json"), manifest)
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let mut command = Command::new(engine);
    command
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
        .env("HOME", &laid.root)
        .current_dir(if world.quirk == Some(Quirk::NotARepository) {
            &laid.root
        } else {
            &laid.repo
        })
        .env("RUNNER_TEMP", &laid.temp)
        .env("VELNOR_INTERNAL_OP", "resolve-qualification-v1")
        .env("VELNOR_REQUEST_FILE", &laid.request)
        .env("VELNOR_FAKE_API", &api_dir)
        .env("GH_TOKEN", "token")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_EVENT_PATH", &laid.payload);
    context_env(world, &mut command);
    for name in &world.unset {
        command.env_remove(name);
    }
    let output = timed(&mut command);
    let requests = fs::read_to_string(api_dir.join("requests.log"))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    let outcome = Outcome {
        error: error_of(&output),
        requests,
        admission: admission_of(&laid.temp),
    };
    fs::remove_dir_all(&laid.root).unwrap_or_else(|e| unreachable!("{e}"));
    outcome
}
