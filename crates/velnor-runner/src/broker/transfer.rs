// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;

use anyhow::Result;

use super::facts::TrustedFacts;
use super::git::{git, git_args, has_tree};

#[derive(Clone, Copy)]
pub(super) enum Transfer {
    Incremental,
    Refetch,
}

pub(super) async fn fetch_wanted(
    mirror: &Path,
    clone_url: &str,
    facts: &TrustedFacts,
    env: &[(OsString, OsString)],
    transfer: Transfer,
) -> Result<()> {
    let mut fetched = fetch_refs(mirror, clone_url, facts.refspecs(), env, transfer).await;
    if !has_tree(mirror, facts.sha.as_str()).await
        && let Some(spec) = facts.sha_refspec()
    {
        fetched = fetch_refs(mirror, clone_url, vec![spec], env, transfer).await;
    }
    fetched
}

async fn fetch_refs(
    mirror: &Path,
    clone_url: &str,
    specs: Vec<String>,
    env: &[(OsString, OsString)],
    transfer: Transfer,
) -> Result<()> {
    let refetch: &[&str] = match transfer {
        Transfer::Incremental => &[],
        Transfer::Refetch => &["--refetch"],
    };
    let base = [
        "-c",
        "protocol.version=2",
        "-c",
        "gc.auto=0",
        "fetch",
        "--no-auto-maintenance",
        "--prune",
        "--no-tags",
        "--quiet",
    ];
    let mut args = git_args(
        Some(mirror),
        &[&base[..], refetch, &["--", clone_url]].concat(),
    );
    args.extend(specs);
    git(&args, env).await
}
