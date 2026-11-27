// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use super::refetch::{Refetch, refetch};
use super::{open, remove_ref_locks};
use crate::broker::facts::{CommitId, TrustedFacts};
use crate::broker::git::{fetch_env, git, git_args, output};
use crate::broker::repair::drop_broken_refs;
use crate::broker::transfer::{Transfer, fetch_trusted};
use crate::broker::trusted::TrustedRef;

enum Blob {
    Readable,
    Absent,
    Missing,
}

async fn tip(mirror: &Path) -> Result<CommitId> {
    let args = git_args(
        Some(mirror),
        &["rev-parse", "--verify", "--quiet", "FETCH_HEAD^{commit}"],
    );
    let found = output(&args, None).await?;
    if !found.succeeded {
        bail!("the fetch returned no commit: {}", found.stderr);
    }
    Ok(CommitId::try_from(found.stdout.trim().to_owned())?)
}

async fn is_ancestor(mirror: &Path, pin: &CommitId, tip: &CommitId) -> bool {
    let args = ["merge-base", "--is-ancestor", pin.as_str(), tip.as_str()];
    git(&git_args(Some(mirror), &args), &[]).await.is_ok()
}

async fn blob(mirror: &Path, pin: &CommitId, path: &str) -> Blob {
    let listing = git_args(Some(mirror), &["ls-tree", pin.as_str(), "--", path]);
    let Ok(listed) = output(&listing, None).await else {
        return Blob::Missing;
    };
    if !listed.succeeded {
        return Blob::Missing;
    }
    if listed.stdout.trim().is_empty() {
        return Blob::Absent;
    }
    let object = format!("{}:{path}", pin.as_str());
    match git(&git_args(Some(mirror), &["cat-file", "blob", &object]), &[]).await {
        Ok(()) => Blob::Readable,
        Err(_) => Blob::Missing,
    }
}

fn absent(pin: &CommitId, path: &str) -> anyhow::Error {
    anyhow!("setup-velnor: the pin {} has no {path}", pin.as_str())
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
    let tip = tip(mirror).await?;
    if !is_ancestor(mirror, pin, &tip).await {
        bail!(
            "setup-velnor: the pin {} is not reachable from {}",
            pin.as_str(),
            trusted.remote()
        );
    }
    match blob(mirror, pin, path).await {
        Blob::Readable => return Ok(()),
        Blob::Absent => return Err(absent(pin, path)),
        Blob::Missing => {}
    }
    refetch(mirror, clone_url, Refetch::Pin(trusted), env)
        .await
        .with_context(|| {
            format!(
                "setup-velnor: repair the mirror for the pin {}",
                pin.as_str()
            )
        })?;
    match blob(mirror, pin, path).await {
        Blob::Readable => Ok(()),
        Blob::Absent => Err(absent(pin, path)),
        Blob::Missing => bail!(
            "setup-velnor: the pin {} has no readable {path} after one repair of the mirror",
            pin.as_str()
        ),
    }
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
    verify(&mirror, &facts.clone_url(), &trusted, pin, path, &env).await?;
    Ok(notices)
}
