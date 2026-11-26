// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/git_mirror.rs

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::process::Command;
use url::Url;

const HEADS: &str = "+refs/heads/*:refs/heads/*";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MirrorWant {
    sha: String,
    pull_merge: Option<u64>,
}

impl MirrorWant {
    pub(crate) fn parse(sha: &str, git_ref: &str) -> Result<Self> {
        let hex = sha.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !hex || !matches!(sha.len(), 40 | 64) {
            bail!("checkout: '{sha}' is not a commit id");
        }
        let pull_merge = git_ref
            .strip_prefix("refs/pull/")
            .and_then(|rest| rest.strip_suffix("/merge"))
            .and_then(|number| number.parse().ok());
        Ok(Self {
            sha: sha.to_ascii_lowercase(),
            pull_merge,
        })
    }

    pub(crate) fn sha(&self) -> &str {
        &self.sha
    }

    fn refspecs(&self) -> Vec<String> {
        let mut specs = vec![HEADS.to_owned()];
        if let Some(number) = self.pull_merge {
            specs.push(format!("+{}:refs/pull/{number}/merge", self.sha));
        }
        specs
    }
}

pub(crate) fn mirror_path(store: &Path, url: &Url, repository: &str) -> Result<PathBuf> {
    let bad = || anyhow::anyhow!("checkout: repository '{repository}' is not owner/name");
    let (owner, name) = repository.split_once('/').ok_or_else(bad)?;
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
    let mut path = store.to_owned();
    for part in [host.as_str(), owner, name] {
        let valid = !part.is_empty()
            && !part.starts_with('.')
            && part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            });
        if !valid {
            return Err(bad());
        }
        path.push(part.to_ascii_lowercase());
    }
    path.as_mut_os_string().push(".git");
    Ok(path)
}

fn auth_env(url: &Url, token: Option<&str>) -> Vec<(String, String)> {
    let mut env = vec![("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned())];
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

async fn git(args: &[&str], env: &[(String, String)]) -> Result<String> {
    let output = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
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
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("path {} is not UTF-8", path.display()))
}

async fn has_commit(mirror: &str, sha: &str) -> bool {
    let object = format!("{sha}^{{commit}}");
    git(&["-C", mirror, "cat-file", "-e", &object], &[])
        .await
        .is_ok()
}

async fn lock(path: &Path) -> Result<std::fs::File> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        file.lock()
            .with_context(|| format!("lock {}", path.display()))?;
        Ok(file)
    })
    .await
    .context("wait for the mirror lock")?
}

const MIRROR_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tbare = true\n";

async fn reset_config(mirror: &Path) -> Result<()> {
    let config = mirror.join("config");
    let staged = mirror.join("config.velnor");
    tokio::fs::write(&staged, MIRROR_CONFIG)
        .await
        .with_context(|| format!("write {}", staged.display()))?;
    tokio::fs::rename(&staged, &config)
        .await
        .with_context(|| format!("replace {}", config.display()))
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

pub(crate) async fn update_mirror(
    root: &Path,
    clone_url: &str,
    repository: &str,
    token: Option<&str>,
    want: &MirrorWant,
) -> Result<PathBuf> {
    let url =
        Url::parse(clone_url).with_context(|| format!("checkout: bad clone URL {clone_url}"))?;
    let store = root.join("git");
    tokio::fs::create_dir_all(&store)
        .await
        .with_context(|| format!("create {}", store.display()))?;
    let mirror = mirror_path(&store, &url, repository)?;
    let mut lock_file = mirror.clone().into_os_string();
    lock_file.push(".lock");
    if let Some(parent) = mirror.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let _lock = lock(Path::new(&lock_file)).await?;
    let mirror_arg = text(&mirror)?;
    if !mirror.join("config").is_file() {
        git(&["init", "-q", "--bare", mirror_arg], &[]).await?;
    }
    reset_config(&mirror).await?;
    if has_commit(mirror_arg, want.sha()).await {
        return Ok(mirror);
    }
    remove_ref_locks(&mirror).await?;
    let specs = want.refspecs();
    let mut args = vec![
        "-C",
        mirror_arg,
        "-c",
        "protocol.version=2",
        "-c",
        "gc.auto=0",
        "fetch",
        "--prune",
        "--no-tags",
        "--quiet",
        "--",
        clone_url,
    ];
    args.extend(specs.iter().map(String::as_str));
    let fetched = git(&args, &auth_env(&url, token)).await;
    if has_commit(mirror_arg, want.sha()).await {
        return Ok(mirror);
    }
    fetched?;
    bail!("checkout: commit {} not found in {clone_url}", want.sha())
}

pub(crate) async fn materialize(
    mirror: &Path,
    clone_url: &str,
    sha: &str,
    dest: &Path,
) -> Result<()> {
    let dest_arg = text(dest)?;
    git(&["init", "-q", dest_arg], &[]).await?;
    let objects = std::path::absolute(mirror)
        .with_context(|| format!("resolve {}", mirror.display()))?
        .join("objects");
    let alternates = dest.join(".git/objects/info/alternates");
    if let Some(parent) = alternates.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create {}", parent.display()))?;
    }
    tokio::fs::write(&alternates, format!("{}\n", objects.display()))
        .await
        .with_context(|| format!("write {}", alternates.display()))?;
    git(&["-C", dest_arg, "remote", "add", "origin", clone_url], &[]).await?;
    git(
        &[
            "-C",
            dest_arg,
            "-c",
            "advice.detachedHead=false",
            "checkout",
            "-q",
            "--detach",
            sha,
        ],
        &[],
    )
    .await?;
    Ok(())
}
