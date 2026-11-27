// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};

use super::git::{Output, git, git_args, has_commit, line_per, output};
use super::proto::Window;
use super::walk::{Walk, walk};

const MAX_ROUNDS: usize = 64;

async fn list_refs(mirror: &Path) -> Result<Output> {
    let args = git_args(
        Some(mirror),
        &["for-each-ref", "--format=%(objectname) %(refname)"],
    );
    output(&args, None).await
}

fn parsed_refs(listed: &Output) -> Result<Vec<(String, String)>> {
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

pub(super) async fn refs(mirror: &Path) -> Result<Vec<(String, String)>> {
    parsed_refs(&list_refs(mirror).await?)
}

pub(super) async fn holds_commit(mirror: &Path, sha: &str) -> bool {
    has_commit(mirror, sha).await
        || refs(mirror)
            .await
            .is_ok_and(|refs| refs.iter().any(|(id, _)| id == sha))
}

fn malformed_packed_refs(stderr: &str, packed: &Path) -> bool {
    ["unexpected line in ", "unterminated line in "]
        .iter()
        .any(|form| {
            let prefix = format!("fatal: {form}{}: ", packed.display());
            stderr.lines().any(|line| line.starts_with(&prefix))
        })
}

async fn refs_past_malformed_packed_refs(
    mirror: &Path,
) -> Result<(Vec<(String, String)>, Vec<String>)> {
    let listed = list_refs(mirror).await?;
    let packed = mirror.join("packed-refs");
    if listed.succeeded || !malformed_packed_refs(&listed.stderr, &packed) {
        return Ok((parsed_refs(&listed)?, Vec::new()));
    }
    if !tokio::fs::symlink_metadata(&packed)
        .await
        .is_ok_and(|meta| meta.is_file())
    {
        bail!("git for-each-ref: {}", listed.stderr);
    }
    quarantine(mirror, "packed-refs", &packed).await?;
    let notice = "mirror: moved the unreadable packed-refs to quarantine".to_owned();
    Ok((refs(mirror).await?, vec![notice]))
}

pub(super) async fn drop_broken_refs(mirror: &Path) -> Result<Vec<String>> {
    let (all, mut notices) = refs_past_malformed_packed_refs(mirror).await?;
    if all.is_empty() {
        return Ok(notices);
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
    for (id, name) in all.iter().filter(|(id, _)| missing.contains(id.as_str())) {
        let delete = git_args(Some(mirror), &["update-ref", "--no-deref", "-d", name, id]);
        git(&delete, &[]).await?;
        notices.push(format!(
            "mirror: removed {name}, which pointed at the missing object {id}"
        ));
    }
    Ok(notices)
}

enum Scope {
    Walked(Walk),
    History,
}

struct Verified {
    succeeded: bool,
    stderr: String,
}

async fn scope(mirror: &Path, sha: &str, window: Window) -> Result<Scope> {
    Ok(match window {
        Window::Last(count) => Scope::Walked(walk(mirror, sha, count).await?),
        Window::All => Scope::History,
    })
}

async fn verify(mirror: &Path, sha: &str, scope: &Scope) -> Result<Verified> {
    let mut words = vec!["rev-list", "--objects", "--verify-objects", "--quiet"];
    let (input, walked) = match scope {
        Scope::Walked(walk) => {
            words.extend(["--no-walk", "--stdin"]);
            let intact = walk.commits.iter().filter(|id| !walk.damaged.contains(*id));
            (Some(line_per(intact.map(String::as_str))), Some(walk))
        }
        Scope::History => {
            words.push(sha);
            (None, None)
        }
    };
    let listed = output(&git_args(Some(mirror), &words), input.as_deref()).await?;
    let (clean, noted) = walked.map_or((true, ""), |walk| (walk.damaged.is_empty(), &walk.stderr));
    Ok(Verified {
        succeeded: listed.succeeded && clean,
        stderr: format!("{noted}\n{}", listed.stderr),
    })
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

fn empty_file_id(path: &str) -> Option<String> {
    let path = Path::new(path);
    let name = path.file_name()?.to_str()?;
    let fan_out = path.parent()?.file_name()?.to_str()?;
    object_id(&format!("{fan_out}{name}"))
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
    if let Some(rest) = message.strip_prefix("object file ") {
        return empty_file_id(rest.strip_suffix(" is empty")?).map(Damage::Loose);
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
        let alone = verify(mirror, id, &Scope::Walked(Walk::commit(id))).await?;
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Scan {
    Sound,
    Unsound,
    Quarantined,
}

async fn was_quarantined(mirror: &Path, id: &str) -> bool {
    let Ok(mut entries) = tokio::fs::read_dir(mirror.join("quarantine")).await else {
        return false;
    };
    let prefix = format!("{id}.");
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            return true;
        }
    }
    false
}

pub(super) async fn quarantine_corrupt(mirror: &Path, sha: &str, window: Window) -> Result<Scan> {
    let mut scan = Scan::Sound;
    let scope = scope(mirror, sha, window).await?;
    for _ in 0..MAX_ROUNDS {
        let verified = verify(mirror, sha, &scope).await?;
        let found = reported(&verified.stderr);
        if verified.succeeded && found.is_empty() {
            return Ok(scan);
        }
        if scan == Scan::Sound {
            scan = Scan::Unsound;
        }
        if let Some(error) = packed_error(&found) {
            return Err(error);
        }
        let files = confirmed_loose(mirror, &found).await?;
        if files.is_empty() {
            break;
        }
        scan = Scan::Quarantined;
        for (id, path) in files {
            quarantine(mirror, &id, &path).await?;
        }
    }
    if scan == Scan::Unsound && was_quarantined(mirror, sha).await {
        scan = Scan::Quarantined;
    }
    Ok(scan)
}
