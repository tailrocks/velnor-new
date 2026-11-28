// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/checkout.rs

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::broker::{
    FailedStep, Request, StoreWriter, TREE_PROBE, TrustedFacts, git_args, mirror_path,
};
use crate::exec::{
    DeclaredInputs, ExecError, InputValue, Inputs, JobCancel, ProcessOutcome, StepAdapter,
    StepContext, StepFuture, StepResult,
};
use crate::store::store_root;

use super::depth::{Depth, js_trim};
use super::git::ask;

pub(super) struct Checkout {
    pub(super) store: Arc<StoreWriter>,
}

impl StepAdapter for Checkout {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with("actions/checkout@")
    }

    fn problem(&self, _reference: &str, inputs: &DeclaredInputs) -> Option<String> {
        self.store.facts().err().or_else(|| match inputs {
            DeclaredInputs::Known(inputs) => validate_inputs(inputs).err(),
            DeclaredInputs::Unknown => Some(
                "actions/checkout cannot validate an expression-valued 'with:' in velnor-runner"
                    .to_owned(),
            ),
        })
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        Box::pin(checkout(&self.store, ctx))
    }
}

fn unsupported_input(name: &str) -> String {
    format!("actions/checkout input '{name}' is not supported by velnor-runner")
}

fn validate_inputs(inputs: &BTreeMap<String, InputValue>) -> Result<(), String> {
    let mut persist_false = false;
    for (name, value) in inputs {
        match (name.to_ascii_lowercase().as_str(), value) {
            ("fetch-depth", InputValue::Literal(depth)) => {
                Depth::parse(depth)?;
            }
            ("fetch-depth", InputValue::Deferred) => {}
            ("persist-credentials", InputValue::Literal(flag)) => {
                persist_false = !js_trim(flag).eq_ignore_ascii_case("true");
            }
            ("persist-credentials", InputValue::Deferred) => persist_false = true,
            _ => return Err(unsupported_input(name)),
        }
    }
    if persist_false {
        Ok(())
    } else {
        Err("actions/checkout requires persist-credentials: false in velnor-runner".to_owned())
    }
}

async fn empty_directory(path: &Path, cancel: Option<&JobCancel>) -> Result<Option<StepResult>> {
    let mut entries = tokio::fs::read_dir(path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    while let Some(entry) = entries.next_entry().await? {
        if cancel.is_some_and(JobCancel::is_cancelled) {
            return Ok(Some(StepResult::Cancelled));
        }
        let target = entry.path();
        if entry.file_type().await?.is_dir() {
            tokio::fs::remove_dir_all(&target).await
        } else {
            tokio::fs::remove_file(&target).await
        }
        .with_context(|| format!("remove {}", target.display()))?;
    }
    Ok(None)
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
    let dir = ctx.workspace.to_path_buf();
    ctx.run_isolated(("git", &args, &dir, &[]), quiet).await
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

fn fetch_depth(inputs: &Inputs) -> Result<Depth, ExecError> {
    Depth::parse(inputs.get("fetch-depth").map_or("", String::as_str))
        .map_err(ExecError::Unsupported)
}

enum Materialized {
    Done,
    Ended(StepResult),
    CheckoutFailed(FailedStep),
}

async fn step(
    ctx: &mut StepContext<'_>,
    args: &[&str],
    failed: FailedStep,
) -> Result<Option<Materialized>, ExecError> {
    Ok(match git(ctx, None, args, false).await? {
        ProcessOutcome::Succeeded => None,
        ProcessOutcome::Cancelled => Some(Materialized::Ended(StepResult::Cancelled)),
        ProcessOutcome::Failed => Some(Materialized::CheckoutFailed(failed)),
    })
}

async fn materialize(
    ctx: &mut StepContext<'_>,
    mirror: &Path,
    facts: &TrustedFacts,
    dest_arg: &str,
    depth: Depth,
) -> Result<Materialized, ExecError> {
    if let Some(ended) = require(ctx, &["init", "-q", dest_arg]).await? {
        return Ok(Materialized::Ended(ended));
    }
    let mirror_abs = std::path::absolute(mirror)
        .with_context(|| format!("resolve {}", mirror.display()))
        .map_err(invalid)?;
    let objects = mirror_abs.join("objects");
    let alternates = Path::new(dest_arg).join(".git/objects/info/alternates");
    tokio::fs::write(&alternates, format!("{}\n", objects.display()))
        .await
        .with_context(|| format!("write {}", alternates.display()))
        .map_err(invalid)?;
    let clone_url = facts.clone_url();
    let remote = ["-C", dest_arg, "remote", "add", "origin", &clone_url];
    if let Some(ended) = require(ctx, &remote).await? {
        return Ok(Materialized::Ended(ended));
    }
    let sha = facts.sha.as_str();
    match depth {
        Depth::Full => {
            let check = [&["-C", dest_arg], TREE_PROBE.as_slice(), &[sha]].concat();
            if let Some(ended) = step(ctx, &check, FailedStep::Other).await? {
                return Ok(ended);
            }
        }
        Depth::Last(1) => {
            let shallow = Path::new(dest_arg).join(".git/shallow");
            tokio::fs::write(&shallow, format!("{sha}\n"))
                .await
                .with_context(|| format!("write {}", shallow.display()))
                .map_err(invalid)?;
        }
        Depth::Last(count) => {
            let source = url::Url::from_file_path(mirror_abs)
                .map_err(|()| invalid("mirror path is not absolute"))?;
            let limit = format!("--depth={count}");
            let fetch = [
                "-C",
                dest_arg,
                "-c",
                "protocol.version=2",
                "-c",
                "core.alternateRefsCommand=true",
                "-c",
                "gc.auto=0",
                "fetch",
                "--no-auto-maintenance",
                "--quiet",
                "--no-tags",
                &limit,
                source.as_str(),
                sha,
            ];
            if let Some(ended) = step(ctx, &fetch, FailedStep::Other).await? {
                return Ok(ended);
            }
        }
    }
    let steps: [(FailedStep, &[&str]); 3] = [
        (FailedStep::ReadTree, &["-C", dest_arg, "read-tree", sha]),
        (
            FailedStep::CheckoutIndex,
            &["-C", dest_arg, "checkout-index", "-a", "-f", "-u"],
        ),
        (
            FailedStep::Other,
            &["-C", dest_arg, "update-ref", "--no-deref", "HEAD", sha],
        ),
    ];
    for (failed, args) in steps {
        if let Some(ended) = step(ctx, args, failed).await? {
            return Ok(ended);
        }
    }
    Ok(Materialized::Done)
}

async fn populate(
    store: &StoreWriter,
    facts: &TrustedFacts,
    depth: Depth,
    ctx: &mut StepContext<'_>,
) -> Result<StepResult, ExecError> {
    let dest = ctx
        .workspace
        .to_str()
        .ok_or_else(|| invalid("workspace is not UTF-8"))?
        .to_owned();
    let mirror = mirror_path(&store_root(), facts).map_err(invalid)?;
    let tree = format!("{}^{{tree}}", facts.sha.as_str());
    let probe = [TREE_PROBE.as_slice(), &[tree.as_str()]].concat();
    let present = git(ctx, Some(&mirror), &probe, true).await?;
    if present == ProcessOutcome::Cancelled {
        return Ok(StepResult::Cancelled);
    }
    if present == ProcessOutcome::Failed || depth.fetches_through_the_mirror_refs() {
        ask(store, ctx, |token| Request::FetchMirror { token }).await?;
    }
    for guarded in [&store_root().join("git"), ctx.temp] {
        ensure_disjoint(ctx.workspace, guarded).map_err(invalid)?;
    }
    if let Some(ended) = empty_directory(ctx.workspace, ctx.cancel.as_ref())
        .await
        .map_err(invalid)?
    {
        return Ok(ended);
    }
    let mut state = materialize(ctx, &mirror, facts, &dest, depth).await?;
    if let Materialized::CheckoutFailed(failed) = state {
        ask(store, ctx, |token| Request::RepairMirror {
            token,
            window: depth.window(),
            failed,
        })
        .await?;
        if let Some(ended) = empty_directory(ctx.workspace, ctx.cancel.as_ref())
            .await
            .map_err(invalid)?
        {
            return Ok(ended);
        }
        state = materialize(ctx, &mirror, facts, &dest, depth).await?;
    }
    match state {
        Materialized::Done => Ok(StepResult::Succeeded),
        Materialized::Ended(ended) => Ok(ended),
        Materialized::CheckoutFailed(_) => Err(ExecError::Invalid(
            "checkout: git checkout failed".to_owned(),
        )),
    }
}

async fn checkout(store: &StoreWriter, ctx: &mut StepContext<'_>) -> Result<StepResult, ExecError> {
    let evaluated = ctx
        .inputs
        .iter()
        .map(|(name, value)| (name.clone(), InputValue::Literal(value.clone())))
        .collect();
    validate_inputs(&evaluated).map_err(ExecError::Unsupported)?;
    let depth = fetch_depth(&ctx.inputs)?;
    let facts = store.facts().map_err(ExecError::Invalid)?;
    let result = populate(store, facts, depth, ctx).await;
    if matches!(result, Ok(StepResult::Succeeded)) {
        ctx.set_output("commit", facts.sha.as_str());
        ctx.set_output("ref", facts.git_ref());
    }
    result
}
