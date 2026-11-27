// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};

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

pub(super) async fn drop_broken_refs(mirror: &Path) -> Result<Vec<String>> {
    let all = refs(mirror).await?;
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let ids = all
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
    for (id, name) in all.iter().filter(|(id, _)| missing.contains(id.as_str())) {
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
        &[
            "rev-list",
            "--objects",
            "--verify-objects",
            "--no-walk",
            "--quiet",
            sha,
        ],
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

enum Damage {
    Loose(String),
    Packed { id: String, pack: String },
}

fn first_id(text: &str) -> Option<String> {
    text.split_whitespace().next().and_then(object_id)
}

fn damage(line: &str) -> Option<Damage> {
    let message = line
        .strip_prefix("error: ")
        .or_else(|| line.strip_prefix("fatal: "))?;
    if let Some(rest) = message.strip_prefix("unable to unpack ") {
        return rest
            .ends_with(" header")
            .then(|| first_id(rest))?
            .map(Damage::Loose);
    }
    if let Some(rest) = message.strip_prefix("loose object ") {
        return rest
            .ends_with(" is corrupt")
            .then(|| first_id(rest))?
            .map(Damage::Loose);
    }
    if let Some(rest) = message.strip_prefix("hash mismatch ") {
        return first_id(rest).map(Damage::Loose);
    }
    let rest = message.strip_prefix("packed object ")?;
    let id = first_id(rest)?;
    let stored = rest
        .split_once("(stored in ")?
        .1
        .strip_suffix(") is corrupt")?;
    let pack = Path::new(stored)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some(Damage::Packed { id, pack })
}

fn reported(report: &str) -> Vec<Damage> {
    report.lines().filter_map(damage).collect()
}

fn packed_error(found: &[Damage]) -> Option<anyhow::Error> {
    found.iter().find_map(|found| match found {
        Damage::Packed { id, pack } => Some(anyhow!(
            "packed object {id} in {pack} is corrupt; the mirror needs an operator rebuild"
        )),
        Damage::Loose(_) => None,
    })
}

pub(super) async fn refuse_packed(mirror: &Path, sha: &str) -> Result<()> {
    let verified = verify(mirror, sha).await?;
    packed_error(&reported(&verified.stderr)).map_or(Ok(()), Err)
}

async fn confirmed_loose(mirror: &Path, found: &[Damage]) -> Result<Vec<(String, PathBuf)>> {
    let ids: BTreeSet<&str> = found
        .iter()
        .filter_map(|found| match found {
            Damage::Loose(id) => Some(id.as_str()),
            Damage::Packed { .. } => None,
        })
        .collect();
    let mut files = Vec::new();
    for id in ids {
        let path = mirror.join("objects").join(&id[..2]).join(&id[2..]);
        let is_file = tokio::fs::symlink_metadata(&path)
            .await
            .is_ok_and(|meta| meta.is_file());
        if !is_file {
            continue;
        }
        let alone = verify(mirror, id).await?;
        let again = reported(&alone.stderr)
            .iter()
            .any(|found| matches!(found, Damage::Loose(again) if again == id));
        if again {
            files.push((id.to_owned(), path));
        }
    }
    Ok(files)
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
        let found = reported(&verified.stderr);
        if let Some(error) = packed_error(&found) {
            return Err(error);
        }
        let files = confirmed_loose(mirror, &found).await?;
        if files.is_empty() {
            break;
        }
        for (id, path) in files {
            quarantine(mirror, &id, &path).await?;
        }
    }
    Ok(damaged)
}
