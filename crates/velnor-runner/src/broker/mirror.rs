// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/git_mirror.rs

use std::fs::TryLockError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::process::Command;
use url::Url;

use super::facts::TrustedFacts;

pub(crate) const GIT_ENV: [(&str, &str); 3] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

pub(crate) fn git_args(git_dir: Option<&Path>, args: &[&str]) -> Vec<String> {
    let mut all = vec![
        "-c".to_owned(),
        "core.hooksPath=/dev/null".to_owned(),
        "--no-replace-objects".to_owned(),
    ];
    if let Some(dir) = git_dir {
        all.push(format!("--git-dir={}", dir.display()));
    }
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    all
}

pub(crate) fn mirror_path(root: &Path, facts: &TrustedFacts) -> Result<PathBuf> {
    let url = facts.server_url.url();
    let host = url
        .host_str()
        .map(|host| host.trim_matches(['[', ']']).replace(':', "+"))
        .filter(|host| !host.is_empty())
        .map_or_else(
            || "_local".to_owned(),
            |host| match url.port() {
                Some(port) => format!("{host}+{port}"),
                None => host,
            },
        );
    let valid = !host.starts_with('.')
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if !valid {
        bail!("mirror: server host '{host}' is not a valid path part");
    }
    let (owner, name) = facts.repository.owner_and_name();
    let mut path = root.join("git");
    for part in [host.as_str(), owner, name] {
        path.push(part.to_ascii_lowercase());
    }
    path.as_mut_os_string().push(".git");
    Ok(path)
}

fn auth_env(url: &Url, token: Option<&str>) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(token) = token.filter(|token| !token.is_empty())
        && url.scheme() == "https"
    {
        let credential = STANDARD.encode(format!("x-access-token:{token}"));
        env.extend([
            ("GIT_CONFIG_COUNT".to_owned(), "1".to_owned()),
            (
                "GIT_CONFIG_KEY_0".to_owned(),
                format!("http.{}/.extraheader", url.origin().ascii_serialization()),
            ),
            (
                "GIT_CONFIG_VALUE_0".to_owned(),
                format!("AUTHORIZATION: basic {credential}"),
            ),
        ]);
    }
    env
}

async fn git(args: &[String], env: &[(String, String)]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .envs(GIT_ENV)
        .env("HOME", "/dev/null")
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("start git")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {}: {}", args.join(" "), stderr.trim());
    }
    Ok(())
}

async fn has_commit(git_dir: &Path, sha: &str) -> bool {
    let object = format!("{sha}^{{commit}}");
    git(&git_args(Some(git_dir), &["cat-file", "-e", &object]), &[])
        .await
        .is_ok()
}

async fn lock(path: &Path) -> Result<std::fs::File> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?
        .into_std()
        .await;
    let mut backoff = Duration::from_millis(5);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_millis(100));
            }
            Err(TryLockError::Error(error)) => {
                return Err(error).with_context(|| format!("lock {}", path.display()));
            }
        }
    }
}

const MIRROR_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tbare = true\n";
const MIRROR_HEAD: &str = "ref: refs/heads/main\n";

async fn remove_entry(path: &Path) -> Result<()> {
    let removed = match tokio::fs::symlink_metadata(path).await {
        Ok(meta) if meta.is_dir() => tokio::fs::remove_dir_all(path).await,
        Ok(_) => tokio::fs::remove_file(path).await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    };
    removed.with_context(|| format!("remove {}", path.display()))
}

async fn install(mirror: &Path, name: &str, body: &str) -> Result<()> {
    let target = mirror.join(name);
    let staged = mirror.join(format!("{name}.velnor"));
    remove_entry(&staged).await?;
    let write = {
        let staged = staged.clone();
        let body = body.to_owned();
        move || -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staged)?;
            file.write_all(body.as_bytes())
        }
    };
    tokio::task::spawn_blocking(write)
        .await
        .context("wait for the mirror write")?
        .with_context(|| format!("write {}", staged.display()))?;
    tokio::fs::rename(&staged, &target)
        .await
        .with_context(|| format!("replace {}", target.display()))
}

fn valid_head(head: &str) -> bool {
    head.strip_prefix("ref: refs/heads/")
        .and_then(|name| name.strip_suffix('\n'))
        .is_some_and(|name| {
            !name.is_empty()
                && !name.contains("..")
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')
                })
        })
}

async fn reset_mirror(mirror: &Path) -> Result<()> {
    remove_entry(&mirror.join("commondir")).await?;
    remove_entry(&mirror.join(".git")).await?;
    let head = tokio::fs::read_to_string(mirror.join("HEAD")).await;
    let is_file = tokio::fs::symlink_metadata(mirror.join("HEAD"))
        .await
        .is_ok_and(|meta| meta.is_file());
    if !is_file || !head.is_ok_and(|head| valid_head(&head)) {
        install(mirror, "HEAD", MIRROR_HEAD).await?;
    }
    install(mirror, "config", MIRROR_CONFIG).await
}

async fn remove_ref_locks(mirror: &Path) -> Result<()> {
    let mut pending = vec![mirror.join("refs")];
    while let Some(dir) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("read {}", dir.display())),
        };
        while let Some(entry) = entries.next_entry().await? {
            let kind = entry.file_type().await?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "lock") {
                tokio::fs::remove_file(&path)
                    .await
                    .with_context(|| format!("remove {}", path.display()))?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn fetch(root: &Path, facts: &TrustedFacts, token: Option<&str>) -> Result<()> {
    let mirror = mirror_path(root, facts)?;
    let mut lock_file = mirror.clone().into_os_string();
    lock_file.push(".lock");
    if let Some(parent) = mirror.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let _lock = lock(Path::new(&lock_file)).await?;
    if !mirror.join("config").is_file() {
        let mirror_arg = mirror
            .to_str()
            .with_context(|| format!("path {} is not UTF-8", mirror.display()))?;
        git(&git_args(None, &["init", "-q", "--bare", mirror_arg]), &[]).await?;
    }
    reset_mirror(&mirror).await?;
    let sha = facts.sha.as_str();
    if has_commit(&mirror, sha).await {
        return Ok(());
    }
    remove_ref_locks(&mirror).await?;
    let clone_url = facts.clone_url();
    let specs = facts.refspecs();
    let mut args = git_args(
        Some(&mirror),
        &[
            "-c",
            "protocol.version=2",
            "-c",
            "gc.auto=0",
            "fetch",
            "--prune",
            "--no-tags",
            "--quiet",
            "--",
            &clone_url,
        ],
    );
    args.extend(specs);
    let fetched = git(&args, &auth_env(facts.server_url.url(), token)).await;
    if has_commit(&mirror, sha).await {
        return Ok(());
    }
    fetched?;
    bail!("checkout: commit {sha} not found in {clone_url}")
}
