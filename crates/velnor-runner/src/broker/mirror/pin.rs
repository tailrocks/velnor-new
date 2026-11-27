// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::refetch::{Refetch, refetch};
use super::{open, remove_ref_locks};
use crate::broker::facts::{CommitId, TrustedFacts};
use crate::broker::git::{fetch_env, git, git_args};
use crate::broker::repair::drop_broken_refs;
use crate::broker::transfer::{Transfer, fetch_trusted};
use crate::broker::trusted::TrustedRef;

async fn is_ancestor(mirror: &Path, pin: &CommitId) -> bool {
    let args = [
        "merge-base",
        "--is-ancestor",
        pin.as_str(),
        TrustedRef::local(),
    ];
    git(&git_args(Some(mirror), &args), &[]).await.is_ok()
}

async fn is_readable(mirror: &Path, pin: &CommitId, path: &str) -> bool {
    let blob = format!("{}:{path}", pin.as_str());
    git(&git_args(Some(mirror), &["cat-file", "blob", &blob]), &[])
        .await
        .is_ok()
}

async fn verify(
    mirror: &Path,
    clone_url: &str,
    trusted: &TrustedRef,
    pin: &CommitId,
    path: &str,
    env: &[(OsString, OsString)],
) -> Result<()> {
    fetch_trusted(mirror, clone_url, trusted, env, Transfer::Incremental)
        .await
        .with_context(|| format!("fetch {}", trusted.remote()))?;
    if !is_ancestor(mirror, pin).await {
        bail!(
            "setup-velnor: the pin {} is not reachable from {}",
            pin.as_str(),
            trusted.remote()
        );
    }
    if is_readable(mirror, pin, path).await {
        return Ok(());
    }
    refetch(mirror, clone_url, Refetch::Pin(trusted), env)
        .await
        .with_context(|| {
            format!(
                "setup-velnor: repair the mirror for the pin {}",
                pin.as_str()
            )
        })?;
    if is_readable(mirror, pin, path).await {
        return Ok(());
    }
    bail!(
        "setup-velnor: the pin {} has no readable {path} after one repair of the mirror",
        pin.as_str()
    )
}

pub(crate) async fn verify_pin(
    root: &Path,
    facts: &TrustedFacts,
    token: Option<&str>,
    pin: &CommitId,
    path: &str,
) -> Result<Vec<String>> {
    let trusted = TrustedRef::of(facts).map_err(|reason| anyhow!(reason))?;
    let (mirror, _lock) = open(root, facts).await?;
    remove_ref_locks(&mirror).await?;
    let notices = drop_broken_refs(&mirror).await?;
    let env = fetch_env(facts.server_url.url(), token);
    let verified = verify(&mirror, &facts.clone_url(), &trusted, pin, path, &env).await;
    let released = git(
        &git_args(Some(&mirror), &["update-ref", "-d", TrustedRef::local()]),
        &[],
    )
    .await
    .context("remove the trusted ref");
    verified?;
    released?;
    Ok(notices)
}
