// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::fs::Permissions;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::env::{DockerCli, Engine, Environment, RUNNER_LABELS, program, words};
use crate::cli::{BuildArgs, JitArgs};
use crate::images::{HOST_BINARY, HOST_BUILD, Lock, RELEASE_IDENTITY, TARGETS, plan, refuse_dirty};
use crate::jit::{pack, register_script};
use crate::side::Side;

const IMAGE: &str = "velnor-local/pair-runner";

#[derive(Debug)]
pub struct Minted {
    pub name: String,
    pub jit: String,
    pub took: Duration,
}

#[derive(Debug)]
pub struct MintError {
    pub name: String,
    pub source: io::Error,
}

impl std::fmt::Display for MintError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "registering {}: {}", self.name, self.source)
    }
}

impl std::error::Error for MintError {}

impl From<MintError> for io::Error {
    fn from(error: MintError) -> Self {
        Self::other(error.to_string())
    }
}

#[must_use]
pub fn jit_file(dir: &Path, side: Side) -> PathBuf {
    match side {
        Side::Trusted => dir.join("jit"),
        Side::Untrusted => dir.join("jit-fork"),
    }
}

pub(super) fn random_hex(bytes: usize) -> io::Result<String> {
    let mut random = vec![0_u8; bytes];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| io::Error::other("no random bytes"))?;
    Ok(random.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

pub async fn mint<E: Engine>(env: &Environment<E>, side: Side) -> Result<Minted, MintError> {
    mint_named(env, side, None).await
}

pub async fn mint_named<E: Engine>(
    env: &Environment<E>,
    side: Side,
    chosen: Option<&str>,
) -> Result<Minted, MintError> {
    let name = match chosen {
        Some(name) => name.to_owned(),
        None => format!(
            "{}-jit-{}",
            env.name,
            random_hex(4).map_err(|e| MintError {
                name: String::new(),
                source: e
            })?
        ),
    };
    let failed = |source: io::Error| MintError {
        name: name.clone(),
        source,
    };
    let label = format!("velnor-local={}", env.name);
    let network = env.network(side);
    let script = register_script(&name, RUNNER_LABELS);
    let started = Instant::now();
    let output = env
        .engine()
        .docker(
            &[
                &words("run --rm --platform linux/arm64 -e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1 --entrypoint sh --label")[..],
                &[&label, "--network", &network, IMAGE, "-c", &script],
            ]
            .concat(),
        )
        .await
        .map_err(failed)?;
    let jit = pack(&output).map_err(|error| failed(io::Error::other(error)))?;
    Ok(Minted {
        name: name.clone(),
        jit,
        took: started.elapsed(),
    })
}

pub(super) async fn private_folder(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .await?;
    fs::set_permissions(path, Permissions::from_mode(0o700)).await
}

pub async fn write_private(path: &Path, content: &str) -> io::Result<()> {
    let fresh = path.with_extension(format!("{}.new", random_hex(8)?));
    let written = stage_and_rename(&fresh, path, content).await;
    if written.is_err() {
        let _ = fs::remove_file(&fresh).await;
    }
    written
}

async fn stage_and_rename(fresh: &Path, path: &Path, content: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(fresh)
        .await?;
    file.write_all(content.as_bytes()).await?;
    file.flush().await?;
    drop(file);
    fs::rename(fresh, path).await
}

pub async fn ensure_clean(root: &Path) -> io::Result<()> {
    let changed = program(
        "git",
        &[
            "-C",
            &root.display().to_string(),
            "status",
            "--porcelain",
            "--untracked-files=no",
        ],
        None,
    )
    .await?;
    if changed.is_empty() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{} has uncommitted changes to tracked files, so the images would claim a revision that does not hold their bytes; commit or stash them first",
        root.display()
    )))
}

pub async fn jit(args: JitArgs) -> io::Result<()> {
    let env = Environment::at(&args.dir, DockerCli).await?;
    let minted = mint(&env, args.side).await?;
    write_private(&jit_file(&env.dir, args.side), &minted.jit).await?;
    println!(
        "minted {} in {:.1} s",
        minted.name,
        minted.took.as_secs_f64()
    );
    Ok(())
}

async fn streamed(args: &[String]) -> io::Result<()> {
    let status = Command::new("docker")
        .args(args)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .await?;
    if status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "docker {} failed",
        args[..2].join(" ")
    )))
}

pub async fn build(args: BuildArgs) -> io::Result<()> {
    let root = fs::canonicalize(&args.root).await?;
    let lock = fs::read_to_string(root.join(".velnor/runner.lock")).await?;
    let lock = Lock::parse(&lock).map_err(io::Error::other)?;
    let git_root = root.display().to_string();
    let dirty = program(
        "git",
        &[
            "-C",
            &git_root,
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            "crates/velnor-runner",
            "images",
            ".velnor",
        ],
        None,
    )
    .await?;
    refuse_dirty(&String::from_utf8_lossy(&dirty)).map_err(io::Error::other)?;
    ensure_clean(&root).await?;
    let sha = program("git", &["-C", &git_root, "rev-parse", "HEAD"], None).await?;
    let sha = String::from_utf8_lossy(&sha).trim().to_owned();
    let stages = std::env::temp_dir().join(format!("velnor-local-build-{}", random_hex(8)?));
    fs::create_dir_all(&stages).await?;
    let built = build_in(&lock, &root, &stages, &sha).await;
    if let Err(error) = fs::remove_dir_all(&stages).await {
        eprintln!(
            "velnor-local: {} was not removed: {error}",
            stages.display()
        );
    }
    built?;
    for tag in TARGETS {
        let id = program(
            "docker",
            &["image", "inspect", "--format", "{{.Id}}", tag],
            None,
        )
        .await?;
        println!("{tag} {}", String::from_utf8_lossy(&id).trim());
    }
    let mut cargo = Command::new("cargo");
    for name in RELEASE_IDENTITY {
        cargo.env_remove(name);
    }
    let status = cargo
        .args(HOST_BUILD)
        .current_dir(&root)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .await?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "cargo {} failed",
            HOST_BUILD.join(" ")
        )));
    }
    println!(
        "built {} (local mode, no release identity)",
        root.join(HOST_BINARY).display()
    );
    Ok(())
}

async fn build_in(lock: &Lock, root: &Path, stages: &Path, sha: &str) -> io::Result<()> {
    for step in plan(lock, root, stages, sha).map_err(io::Error::other)? {
        streamed(&step).await?;
    }
    Ok(())
}
