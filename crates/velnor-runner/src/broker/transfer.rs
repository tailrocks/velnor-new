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
    let (keep_pack, refetch): (&[&str], &[&str]) = match transfer {
        Transfer::Incremental => (&[], &[]),
        Transfer::Refetch => (&["-c", "fetch.unpackLimit=1"], &["--refetch"]),
    };
    let base = [
        "-c",
        "protocol.version=2",
        "-c",
        "gc.auto=0",
        "-c",
        "http.lowSpeedLimit=1000",
        "-c",
        "http.lowSpeedTime=60",
    ];
    let fetch = [
        "fetch",
        "--no-auto-maintenance",
        "--prune",
        "--no-tags",
        "--quiet",
    ];
    let mut args = git_args(
        Some(mirror),
        &[&base[..], keep_pack, &fetch, refetch, &["--", clone_url]].concat(),
    );
    args.extend(specs);
    let fetched = git(&args, env).await;
    if fetched.is_err() {
        remove_partial_packs(mirror).await;
    }
    fetched
}

async fn remove_partial_packs(mirror: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(mirror.join("objects/pack")).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let partial = ["tmp_pack_", "tmp_idx_"]
            .iter()
            .any(|prefix| name.to_string_lossy().starts_with(prefix));
        if partial && entry.file_type().await.is_ok_and(|kind| kind.is_file()) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}
