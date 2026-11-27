// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use super::facts::TrustedFacts;
use super::git::{Output, git, git_args, has_commit, output};

const MAX_ROUNDS: usize = 64;

async fn refs(mirror: &Path) -> Result<Vec<(String, String)>> {
    let args = git_args(
        Some(mirror),
        &["for-each-ref", "--format=%(objectname) %(refname)"],
    );
    let listed = output(&args, None).await?;
    if !listed.succeeded {
        bail!("git for-each-ref: {}", listed.stderr);
    }
    Ok(listed
        .stdout
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(id, name)| (id.to_owned(), name.to_owned()))
        .collect())
}

pub(super) async fn holds_commit(mirror: &Path, sha: &str) -> bool {
    has_commit(mirror, sha).await
        || refs(mirror)
            .await
            .is_ok_and(|refs| refs.iter().any(|(id, _)| id == sha))
}

pub(super) async fn drop_broken_refs(mirror: &Path, facts: &TrustedFacts) -> Result<Vec<String>> {
    let outside: Vec<(String, String)> = refs(mirror)
        .await?
        .into_iter()
        .filter(|(_, name)| !facts.writes_ref(name))
        .collect();
    if outside.is_empty() {
        return Ok(Vec::new());
    }
    let ids = outside
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let probe = git_args(Some(mirror), &["cat-file", "--batch-check"]);
    let probed = output(&probe, Some(&format!("{ids}\n"))).await?;
    let missing: BTreeSet<&str> = probed
        .stdout
        .lines()
        .filter_map(|line| line.strip_suffix(" missing"))
        .collect();
    let mut notices = Vec::new();
    for (id, name) in outside
        .iter()
        .filter(|(id, _)| missing.contains(id.as_str()))
    {
        let delete = git_args(Some(mirror), &["update-ref", "--no-deref", "-d", name, id]);
        git(&delete, &[]).await?;
        notices.push(format!(
            "mirror: removed {name}, which pointed at the missing object {id}"
        ));
    }
    Ok(notices)
}

async fn verify(mirror: &Path, sha: &str) -> Result<Output> {
    let args = git_args(
        Some(mirror),
        &["rev-list", "--objects", "--verify-objects", "--quiet", sha],
    );
    output(&args, None).await
}

fn object_id(word: &str) -> Option<String> {
    let word = word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric());
    let hex = word
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    (matches!(word.len(), 40 | 64) && hex).then(|| word.to_owned())
}

async fn loose_files(mirror: &Path, report: &str) -> Vec<(String, std::path::PathBuf)> {
    let ids: BTreeSet<String> = report
        .lines()
        .filter(|line| line.starts_with("error:") || line.starts_with("fatal:"))
        .flat_map(str::split_whitespace)
        .filter_map(object_id)
        .collect();
    let mut found = Vec::new();
    for id in ids {
        let path = mirror.join("objects").join(&id[..2]).join(&id[2..]);
        let is_file = tokio::fs::symlink_metadata(&path)
            .await
            .is_ok_and(|meta| meta.is_file());
        if is_file {
            found.push((id, path));
        }
    }
    found
}

async fn quarantine(mirror: &Path, id: &str, path: &Path) -> Result<()> {
    let dir = mirror.join("quarantine");
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create {}", dir.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let target = dir.join(format!("{id}.{stamp}"));
    tokio::fs::rename(path, &target)
        .await
        .with_context(|| format!("move {} to {}", path.display(), target.display()))
}

pub(super) async fn quarantine_corrupt(mirror: &Path, sha: &str) -> Result<bool> {
    let mut damaged = false;
    for _ in 0..MAX_ROUNDS {
        let verified = verify(mirror, sha).await?;
        if verified.succeeded {
            return Ok(damaged);
        }
        damaged = true;
        let files = loose_files(mirror, &verified.stderr).await;
        if files.is_empty() {
            break;
        }
        for (id, path) in files {
            quarantine(mirror, &id, &path).await?;
        }
    }
    Ok(damaged)
}
