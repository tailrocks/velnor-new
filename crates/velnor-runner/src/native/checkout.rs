// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/checkout.rs

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::broker::{
    GIT_ENV, Request, Response, StoreWriter, Token, TrustedFacts, git_args, mirror_path,
};
use crate::exec::{ExecError, ProcessOutcome, StepAdapter, StepContext, StepFuture, StepResult};
use crate::store::store_root;

pub(super) struct Checkout {
    pub(super) store: StoreWriter,
}

impl StepAdapter for Checkout {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with("actions/checkout@")
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        Box::pin(checkout(&self.store, ctx))
    }
}

fn unsupported_input(name: &str) -> ExecError {
    ExecError::Unsupported(format!(
        "actions/checkout input '{name}' is not supported by velnor-runner"
    ))
}

fn validate_inputs(inputs: &BTreeMap<String, String>) -> Result<(), ExecError> {
    let mut persist_false = false;
    for (name, value) in inputs {
        match name.to_ascii_lowercase().as_str() {
            "fetch-depth" => {
                if value.parse::<u64>().is_err() {
                    return Err(ExecError::Invalid(format!(
                        "actions/checkout input 'fetch-depth' must be a number, got '{value}'"
                    )));
                }
            }
            "persist-credentials" => persist_false = !value.eq_ignore_ascii_case("true"),
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

fn ensure_disjoint(workspace: &Path, guarded: &Path) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace)
        .with_context(|| format!("resolve {}", workspace.display()))?;
    let guarded =
        std::fs::canonicalize(guarded).with_context(|| format!("resolve {}", guarded.display()))?;
    if workspace.starts_with(&guarded) || guarded.starts_with(&workspace) {
        bail!(
            "checkout: workspace {} and {} must not contain each other",
            workspace.display(),
            guarded.display()
        );
    }
    Ok(())
}

fn invalid(error: impl std::fmt::Display) -> ExecError {
    ExecError::Invalid(format!("{error:#}"))
}

async fn git(
    ctx: &mut StepContext<'_>,
    git_dir: Option<&Path>,
    args: &[&str],
    quiet: bool,
) -> Result<ProcessOutcome, ExecError> {
    let args = git_args(git_dir, args);
    let env: Vec<(String, String)> = GIT_ENV
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    let dir = ctx.workspace.to_path_buf();
    ctx.run_process(("git", &args, &dir, &env), quiet).await
}

async fn require(
    ctx: &mut StepContext<'_>,
    args: &[&str],
) -> Result<Option<StepResult>, ExecError> {
    match git(ctx, None, args, false).await? {
        ProcessOutcome::Succeeded => Ok(None),
        ProcessOutcome::Cancelled => Ok(Some(StepResult::Cancelled)),
        ProcessOutcome::Failed => Err(ExecError::Invalid(format!(
            "checkout: git {} failed",
            args.iter().find(|arg| !arg.starts_with('-')).unwrap_or(&"")
        ))),
    }
}

async fn materialize(
    ctx: &mut StepContext<'_>,
    mirror: &Path,
    facts: &TrustedFacts,
) -> Result<Option<StepResult>, ExecError> {
    let dest = ctx.workspace.to_path_buf();
    let dest_arg = dest
        .to_str()
        .ok_or_else(|| invalid("workspace is not UTF-8"))?;
    if let Some(ended) = require(ctx, &["init", "-q", dest_arg]).await? {
        return Ok(Some(ended));
    }
    let objects = std::path::absolute(mirror)
        .with_context(|| format!("resolve {}", mirror.display()))
        .map_err(invalid)?
        .join("objects");
    let alternates = dest.join(".git/objects/info/alternates");
    tokio::fs::write(&alternates, format!("{}\n", objects.display()))
        .await
        .with_context(|| format!("write {}", alternates.display()))
        .map_err(invalid)?;
    let clone_url = facts.clone_url();
    let sha = facts.sha.as_str();
    for args in [
        vec!["-C", dest_arg, "remote", "add", "origin", &clone_url],
        vec![
            "-C",
            dest_arg,
            "-c",
            "advice.detachedHead=false",
            "checkout",
            "-q",
            "--detach",
            sha,
        ],
    ] {
        if let Some(ended) = require(ctx, &args).await? {
            return Ok(Some(ended));
        }
    }
    Ok(None)
}

async fn populate(
    store: &StoreWriter,
    facts: &TrustedFacts,
    ctx: &mut StepContext<'_>,
) -> Result<StepResult, ExecError> {
    let mirror = mirror_path(&store_root(), facts).map_err(invalid)?;
    let object = format!("{}^{{commit}}", facts.sha.as_str());
    let present = git(ctx, Some(&mirror), &["cat-file", "-e", &object], true).await?;
    if present == ProcessOutcome::Cancelled {
        return Ok(StepResult::Cancelled);
    }
    if present == ProcessOutcome::Failed {
        let token = ctx.github_token.clone().map(Token::new);
        match store.request(Request::FetchMirror { token }).await {
            Response::Done => {}
            Response::Failed { message } => return Err(ExecError::Invalid(message)),
            Response::Hello(_) => {
                return Err(ExecError::Invalid(
                    "store broker protocol error: Hello answered a FetchMirror request".to_owned(),
                ));
            }
        }
    }
    for guarded in [&store_root().join("git"), ctx.temp] {
        ensure_disjoint(ctx.workspace, guarded).map_err(invalid)?;
    }
    empty_directory(ctx.workspace).await.map_err(invalid)?;
    Ok(materialize(ctx, &mirror, facts)
        .await?
        .unwrap_or(StepResult::Succeeded))
}

async fn checkout(store: &StoreWriter, ctx: &mut StepContext<'_>) -> Result<StepResult, ExecError> {
    validate_inputs(&ctx.inputs)?;
    let facts = store.facts().map_err(ExecError::Invalid)?;
    let cancel = ctx.cancel.clone();
    let work = populate(store, facts, ctx);
    let result = match cancel {
        Some(cancel) => tokio::select! {
            result = work => result,
            () = cancel.cancelled() => Ok(StepResult::Cancelled),
        },
        None => work.await,
    };
    if matches!(result, Ok(StepResult::Succeeded)) {
        ctx.set_output("commit", facts.sha.as_str());
        ctx.set_output("ref", facts.git_ref());
    }
    result
}
