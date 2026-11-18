// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::checkout::git;
use crate::cli::{EnvironmentArgs, RunnerMode};
use crate::pin::{Sha256, rewrite};

const FLAG: &str = "--velnor-ci-binary";
const MAX_BYTES: u64 = 256 << 20;
const ELF_HEADER: usize = 20;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_LITTLE_ENDIAN: u8 = 1;
const ELF_MACHINE_AARCH64: [u8; 2] = [0xb7, 0];
const TARBALLS: &str =
    "Runner.Server__ActionDownloadUrls__0__TarballUrl=http://api-stub/repos/{0}/tarball/{1}";
const GITHUB: &str = ".github";
const WORKFLOWS: &str = ".github/workflows";
const NO_FOLLOW: i32 = nix::libc::O_NOFOLLOW;
const COMMIT: &str = "velnor-local: pin the local velnor-ci";
const MOUNT_DIR: &str = "/opt/velnor/local";

fn refused(reason: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("{FLAG} {reason}"))
}

async fn checked(path: &Path) -> io::Result<PathBuf> {
    let path = fs::canonicalize(path)
        .await
        .map_err(|error| refused(format!("{}: {error}", path.display())))?;
    let shown = path.display().to_string();
    if shown.contains([':', ',', '\n']) {
        return Err(refused(format!(
            "{shown} has a ':', ',' or newline, which docker -v cannot mount"
        )));
    }
    let meta = fs::metadata(&path).await?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 {
        return Err(refused(format!(
            "{shown} is not an executable regular file"
        )));
    }
    if meta.len() > MAX_BYTES {
        return Err(refused(format!("{shown} is larger than {MAX_BYTES} bytes")));
    }
    let mut header = [0_u8; ELF_HEADER];
    let mut file = fs::File::open(&path).await?;
    let read = file.read_exact(&mut header).await;
    let elf = read.is_ok()
        && header[..4] == ELF_MAGIC
        && header[4] == ELF_CLASS_64
        && header[5] == ELF_LITTLE_ENDIAN
        && header[18..20] == ELF_MACHINE_AARCH64;
    if !elf {
        return Err(refused(format!(
            "{shown} is not a linux-arm64 ELF executable, which is what the runner containers run"
        )));
    }
    Ok(path)
}

/// What `--velnor-ci-binary` adds to the containers, each as docker arguments. Empty without it.
#[derive(Debug, Default)]
pub(super) struct LocalCi {
    /// For the trusted runners: the binary, read-only.
    pub(super) runner: Vec<String>,
    /// For the trusted runner.server: where it sends action downloads.
    pub(super) standin: Vec<String>,
    /// For the api-stub: serve the local variant of setup-velnor.
    pub(super) stub: Vec<String>,
}

async fn binary(args: &EnvironmentArgs) -> io::Result<Option<PathBuf>> {
    let Some(binary) = &args.velnor_ci_binary else {
        return Ok(None);
    };
    if args.runner != RunnerMode::Plain {
        return Err(refused(
            "needs --runner plain: velnor-host owns the pair containers and mounts nothing from the host into them",
        ));
    }
    checked(binary).await.map(Some)
}

async fn real_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path).await {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => Err(refused(format!(
            "{} is not a real directory (a symlink or a special file), so the workflows in this run's copy are not rewritten through it",
            path.display()
        ))),
    }
}

async fn workflow_files(repo: &Path) -> io::Result<Vec<PathBuf>> {
    let github = repo.join(GITHUB);
    let directory = repo.join(WORKFLOWS);
    if !real_directory(&github).await? || !real_directory(&directory).await? {
        return Ok(Vec::new());
    }
    let root = fs::canonicalize(repo).await?;
    let mut entries = fs::read_dir(&directory).await?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let workflow = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yml" | "yaml")
        );
        if !workflow {
            continue;
        }
        let meta = fs::symlink_metadata(&path).await?;
        if meta.is_dir() {
            continue;
        }
        if !meta.is_file() {
            return Err(refused(format!(
                "{} is not a regular file (a symlink or a special file), so it is not rewritten",
                path.display()
            )));
        }
        if !fs::canonicalize(&path).await?.starts_with(&root) {
            return Err(refused(format!(
                "{} resolves outside this run's copy of the checkout",
                path.display()
            )));
        }
        files.push(path);
    }
    Ok(files)
}

async fn read_workflow(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(NO_FOLLOW)
        .open(path)
        .await?
        .read_to_string(&mut text)
        .await?;
    Ok(text)
}

async fn write_workflow(path: &Path, text: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .custom_flags(NO_FOLLOW)
        .open(path)
        .await?;
    file.write_all(text.as_bytes()).await?;
    file.flush().await
}

pub async fn pin_workflows(args: &EnvironmentArgs, repo: &Path, tag: &str) -> io::Result<()> {
    let Some(binary) = binary(args).await? else {
        return Ok(());
    };
    let pin = Sha256::of(&fs::read(&binary).await?);
    let mut pinned = 0;
    for path in workflow_files(repo).await? {
        let (text, count) = rewrite(&read_workflow(&path).await?, &pin);
        if count > 0 {
            write_workflow(&path, &text).await?;
            pinned += count;
        }
    }
    println!(
        "velnor-local {tag}: WARNING: --velnor-ci-binary replaces the pinned velnor-ci release with {} (SHA-256 {}) in this run's copy of the workflows, {pinned} pin(s) rewritten; the integrity check itself runs unchanged, and this run does not test a release",
        binary.display(),
        pin.as_str()
    );
    if pinned > 0 {
        git(repo, &["add", "-A"], None).await?;
        git(repo, &["commit", "-q", "-m", COMMIT], None).await?;
    }
    Ok(())
}

pub(super) async fn flags(args: &EnvironmentArgs) -> io::Result<LocalCi> {
    let Some(binary) = binary(args).await? else {
        return Ok(LocalCi::default());
    };
    Ok(LocalCi {
        runner: vec![
            "-v".to_owned(),
            format!("{}:{MOUNT_DIR}/velnor-ci:ro", binary.display()),
        ],
        standin: vec!["-e".to_owned(), TARBALLS.to_owned()],
        stub: vec!["--local-setup-velnor".to_owned()],
    })
}
