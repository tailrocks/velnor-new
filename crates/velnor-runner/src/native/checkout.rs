// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/checkout.rs

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::git_mirror::{MirrorWant, materialize, update_mirror};
use crate::exec::{ExecError, Inputs, StepAdapter, StepContext, StepFuture, StepResult};
use crate::store::store_root;

pub(super) struct Checkout;

impl StepAdapter for Checkout {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with("actions/checkout@")
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        Box::pin(checkout(ctx))
    }
}

fn unsupported_input(name: &str) -> ExecError {
    ExecError::Unsupported(format!(
        "actions/checkout input '{name}' is not supported by velnor-runner"
    ))
}

fn validate_inputs(inputs: &Inputs) -> Result<(), ExecError> {
    let mut persist_false = false;
    for (name, value) in inputs.iter() {
        match name.to_ascii_lowercase().as_str() {
            "fetch-depth" => {
                if value.parse::<u64>().is_err() {
                    return Err(ExecError::Invalid(format!(
                        "actions/checkout input 'fetch-depth' must be a number, got '{value}'"
                    )));
                }
            }
            "persist-credentials" => match value.as_str() {
                "false" | "False" | "FALSE" => persist_false = true,
                "" | "true" | "True" | "TRUE" => {}
                _ => {
                    return Err(ExecError::Invalid(format!(
                        "actions/checkout input 'persist-credentials' must be a boolean, got '{value}'"
                    )));
                }
            },
            _ => return Err(unsupported_input(name)),
        }
    }
    if persist_false {
        Ok(())
    } else {
        Err(ExecError::Unsupported(
            "actions/checkout requires persist-credentials: false in velnor-runner".to_owned(),
        ))
    }
}

fn variable<'a>(env: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, ExecError> {
    env.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ExecError::Invalid(format!("checkout: {name} is not set")))
}

async fn empty_directory(path: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    while let Some(entry) = entries.next_entry().await? {
        let target = entry.path();
        if entry.file_type().await?.is_dir() {
            tokio::fs::remove_dir_all(&target).await
        } else {
            tokio::fs::remove_file(&target).await
        }
        .with_context(|| format!("remove {}", target.display()))?;
    }
    Ok(())
}

fn ensure_disjoint(workspace: &Path, mirror: &Path) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace)
        .with_context(|| format!("resolve {}", workspace.display()))?;
    let mirror =
        std::fs::canonicalize(mirror).with_context(|| format!("resolve {}", mirror.display()))?;
    if workspace.starts_with(&mirror) || mirror.starts_with(&workspace) {
        bail!(
            "checkout: workspace {} and git mirror {} must not contain each other",
            workspace.display(),
            mirror.display()
        );
    }
    Ok(())
}

struct Target<'a> {
    workspace: &'a Path,
    token: Option<&'a str>,
}

async fn populate(
    target: &Target<'_>,
    clone_url: &str,
    repository: &str,
    want: &MirrorWant,
) -> Result<()> {
    let mirror = update_mirror(&store_root(), clone_url, repository, target.token, want).await?;
    ensure_disjoint(target.workspace, &mirror)?;
    empty_directory(target.workspace).await?;
    materialize(&mirror, clone_url, want.sha(), target.workspace).await
}

async fn checkout(ctx: &mut StepContext<'_>) -> Result<StepResult, ExecError> {
    validate_inputs(&ctx.inputs)?;
    let server = variable(&ctx.env, "GITHUB_SERVER_URL")?;
    let repository = variable(&ctx.env, "GITHUB_REPOSITORY")?;
    let clone_url = format!("{}/{repository}.git", server.trim_end_matches('/'));
    let want = MirrorWant::parse(
        variable(&ctx.env, "GITHUB_SHA")?,
        ctx.env.get("GITHUB_REF").map_or("", String::as_str),
    )
    .map_err(|error| ExecError::Invalid(format!("{error:#}")))?;
    let target = Target {
        workspace: ctx.workspace,
        token: ctx.github_token.as_deref(),
    };
    let work = populate(&target, &clone_url, repository, &want);
    let finished = match ctx.cancel.clone() {
        Some(cancel) => tokio::select! {
            result = work => result,
            () = cancel.cancelled() => return Ok(StepResult::Cancelled),
        },
        None => work.await,
    };
    finished.map_err(|error| ExecError::Invalid(format!("{error:#}")))?;
    Ok(StepResult::Succeeded)
}
