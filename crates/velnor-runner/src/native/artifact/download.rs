// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use futures_util::future::try_join_all;

use super::blob::Fetched;
use super::inputs::DownloadInputs;
use super::transfer::fetch_blob;
use super::twirp::{Client, Listed};
use super::zip::{MAX_DOWNLOAD_BYTES, TempZip, install};
use super::{Blocking, client, failed, search, validate_download_name, validate_name};
use crate::exec::{ExecError, StepContext, StepResult};

const PARALLEL_DOWNLOADS: usize = 5;

fn newest_per_name(listed: Vec<Listed>) -> Vec<Listed> {
    let mut newest: BTreeMap<String, Listed> = BTreeMap::new();
    for artifact in listed {
        match newest.get(&artifact.name) {
            Some(known) if known.id >= artifact.id => {}
            _ => {
                newest.insert(artifact.name.clone(), artifact);
            }
        }
    }
    let mut artifacts: Vec<Listed> = newest.into_values().collect();
    artifacts.sort_by_key(|artifact| std::cmp::Reverse(artifact.id));
    artifacts
}

async fn fetch(client: &Client, artifact: &Listed, temp: &Path) -> Result<Fetched, ExecError> {
    let url = client.signed_url(artifact).await.map_err(|e| failed(&e))?;
    fetch_blob(&url, temp, MAX_DOWNLOAD_BYTES)
        .await
        .map_err(|error| failed(&error))
}

fn lexical(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
}

fn digest_matches(expected: Option<&str>, actual: &str) -> bool {
    expected.is_none_or(|expected| {
        expected
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.eq_ignore_ascii_case(actual))
    })
}

async fn download(
    ctx: &mut StepContext<'_>,
    inputs: &DownloadInputs,
    blocking: &Blocking,
) -> Result<StepResult, ExecError> {
    let home = ctx.env.get("HOME").map(PathBuf::from);
    let destination = lexical(&search::resolve(
        ctx.workspace,
        home.as_deref(),
        &search::with_home_prefix(home.as_deref(), inputs.path.as_deref().unwrap_or_default()),
    ));
    let client = client(ctx)?;
    let listed = client.list(None).await;
    client.drain_notes(ctx);
    let mut artifacts = newest_per_name(listed.map_err(|e| failed(&e))?);
    if let Some(pattern) = &inputs.pattern {
        artifacts.retain(|artifact| pattern.matches(&artifact.name));
    }
    let single = artifacts.len() == 1;
    for artifact in &artifacts {
        if single {
            validate_name(&artifact.name)?;
        } else {
            validate_download_name(&artifact.name)?;
        }
        ctx.log(&format!("- {} (ID: {})", artifact.name, artifact.id));
    }
    let mut mismatched = Vec::new();
    for batch in artifacts.chunks(PARALLEL_DOWNLOADS) {
        let temps: Vec<TempZip> = batch.iter().map(|_| TempZip::new(ctx.temp)).collect();
        let fetched = try_join_all(
            batch
                .iter()
                .zip(&temps)
                .map(|(artifact, temp)| fetch(&client, artifact, temp.path())),
        )
        .await;
        client.drain_notes(ctx);
        for ((artifact, temp), Fetched { sha256, raw_name }) in
            batch.iter().zip(&temps).zip(fetched?)
        {
            if !digest_matches(artifact.digest.as_deref(), &sha256) {
                mismatched.push(artifact.name.as_str());
                continue;
            }
            let target = if single {
                destination.clone()
            } else {
                destination.join(&artifact.name)
            };
            let archive = temp.path().to_path_buf();
            blocking
                .run(move |stop| install(&archive, &target, raw_name.as_deref(), stop))
                .await?
                .map_err(|error| failed(&error))?;
        }
    }
    if !mismatched.is_empty() {
        return Err(ExecError::Invalid(format!(
            "Digest validation failed for artifact(s): {}",
            mismatched.join(", ")
        )));
    }
    ctx.log(&format!(
        "Total of {} artifact(s) downloaded",
        artifacts.len()
    ));
    ctx.set_output("download-path", destination.to_string_lossy());
    Ok(StepResult::Succeeded)
}

pub(super) async fn run(
    ctx: &mut StepContext<'_>,
    blocking: &Blocking,
) -> Result<StepResult, ExecError> {
    let inputs = DownloadInputs::parse(&ctx.inputs)?;
    download(ctx, &inputs, blocking)
        .await
        .map_err(|error| ExecError::Invalid(format!("Unable to download artifact(s): {error}")))
}
