// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{is_file, missing_tree, remove_entry, sound};
use crate::broker::facts::TrustedFacts;
use crate::broker::git::{git, git_args, has_tree, update};
use crate::broker::repair::refs;
use crate::broker::transfer::{Transfer, fetch_wanted};

const STAGE: &str = "refetch.velnor";

pub(super) async fn adopt_packs(stage: &Path, mirror: &Path) -> Result<()> {
    let from = stage.join("objects/pack");
    let to = mirror.join("objects/pack");
    tokio::fs::create_dir_all(&to)
        .await
        .with_context(|| format!("create {}", to.display()))?;
    let mut entries = tokio::fs::read_dir(&from)
        .await
        .with_context(|| format!("read {}", from.display()))?;
    let mut stems = BTreeSet::new();
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".idx") {
            stems.insert(stem.to_owned());
        }
        names.push(name);
    }
    for stem in stems {
        let index = format!("{stem}.idx");
        if is_file(&to.join(&index)).await && is_file(&to.join(format!("{stem}.pack"))).await {
            continue;
        }
        let sidecars = names
            .iter()
            .filter(|name| name.starts_with(&format!("{stem}.")) && **name != index);
        for name in sidecars.chain([&index]) {
            remove_entry(&to.join(name)).await?;
            tokio::fs::rename(from.join(name), to.join(name))
                .await
                .with_context(|| format!("move {name} into {}", to.display()))?;
        }
    }
    Ok(())
}

async fn update_refs(mirror: &Path, script: &str) -> Result<()> {
    if script.is_empty() {
        return Ok(());
    }
    let args = git_args(Some(mirror), &["update-ref", "--stdin"]);
    let updated = update(&args, script).await?;
    if !updated.succeeded {
        bail!("git update-ref: {}", updated.stderr);
    }
    Ok(())
}

async fn adopt_refs(stage: &Path, mirror: &Path) -> Result<()> {
    let fresh = refs(stage).await?;
    let kept: BTreeSet<&str> = fresh.iter().map(|(_, name)| name.as_str()).collect();
    let mut deletions = String::new();
    for (_, name) in refs(mirror).await? {
        if name.starts_with("refs/heads/") && !kept.contains(name.as_str()) {
            deletions.extend(["delete ", &name, "\n"]);
        }
    }
    let mut updates = String::new();
    for (id, name) in &fresh {
        updates.extend(["update ", name, " ", id, "\n"]);
    }
    update_refs(mirror, &deletions).await?;
    update_refs(mirror, &updates).await
}

async fn adopt(
    mirror: &Path,
    stage: &Path,
    clone_url: &str,
    facts: &TrustedFacts,
    env: &[(OsString, OsString)],
) -> Result<()> {
    let sha = facts.sha.as_str();
    let stage_arg = stage
        .to_str()
        .with_context(|| format!("path {} is not UTF-8", stage.display()))?;
    git(&git_args(None, &["init", "-q", "--bare", stage_arg]), &[]).await?;
    fetch_wanted(stage, clone_url, facts, env, Transfer::Refetch)
        .await
        .map_err(|error| {
            if format!("{error:#}").contains("unknown option") {
                error.context(
                    "repair the mirror with git fetch --refetch, which needs git 2.36 or newer",
                )
            } else {
                error
            }
        })?;
    if !has_tree(stage, sha).await {
        return Err(missing_tree(stage, sha, clone_url).await);
    }
    adopt_packs(stage, mirror).await?;
    adopt_refs(stage, mirror).await
}

pub(super) async fn refetch(
    mirror: &Path,
    clone_url: &str,
    facts: &TrustedFacts,
    env: &[(OsString, OsString)],
) -> Result<()> {
    sound::clear(mirror).await?;
    let stage = mirror.join(STAGE);
    remove_entry(&stage).await?;
    let adopted = adopt(mirror, &stage, clone_url, facts, env).await;
    let cleared = remove_entry(&stage).await;
    adopted?;
    cleared?;
    let _ = git(&git_args(Some(mirror), &["prune-packed", "-q"]), &[]).await;
    Ok(())
}

#[cfg(test)]
mod tests;
