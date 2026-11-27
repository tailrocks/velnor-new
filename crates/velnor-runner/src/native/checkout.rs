// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/checkout.rs

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::broker::{
    Request, Response, StoreWriter, TREE_PROBE, Token, TrustedFacts, git_args, mirror_path,
};
use crate::exec::{
    ExecError, InputValue, ProcessOutcome, StepAdapter, StepContext, StepFuture, StepResult,
};
use crate::store::store_root;

pub(super) struct Checkout {
    pub(super) store: StoreWriter,
}

impl StepAdapter for Checkout {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with("actions/checkout@")
    }

    fn problem(&self, inputs: &BTreeMap<String, InputValue>) -> Option<String> {
        validate_inputs(inputs).err()
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
                if !depth.is_empty() && depth.parse::<u64>().is_err() {
                    return Err(format!(
                        "actions/checkout input 'fetch-depth' must be a number, got '{depth}'"
                    ));
                }
            }
            ("fetch-depth", InputValue::Deferred) => {}
            ("persist-credentials", InputValue::Literal(flag)) => {
                persist_false = !flag.eq_ignore_ascii_case("true");
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
    let dir = ctx.workspace.to_path_buf();
    ctx.run_process(("git", &args, &dir, &[]), quiet).await
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

#[derive(Clone, Copy)]
enum Depth {
    Full,
    Last(u64),
}

fn fetch_depth(inputs: &BTreeMap<String, String>) -> Result<Depth, ExecError> {
    let given = inputs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("fetch-depth"))
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty());
    match given.map(str::parse::<u64>) {
        None => Ok(Depth::Last(1)),
        Some(Ok(0)) => Ok(Depth::Full),
        Some(Ok(depth)) => Ok(Depth::Last(depth)),
        Some(Err(_)) => Err(ExecError::Unsupported(
            "actions/checkout input 'fetch-depth' must be a number".to_owned(),
        )),
    }
}

enum Materialized {
    Done,
    Ended(StepResult),
    CheckoutFailed,
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
        Depth::Full => {}
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
                "fetch",
                "--quiet",
                "--no-tags",
                &limit,
                source.as_str(),
                sha,
            ];
            if let Some(ended) = require(ctx, &fetch).await? {
                return Ok(Materialized::Ended(ended));
            }
        }
    }
    let steps: [&[&str]; 3] = [
        &["-C", dest_arg, "read-tree", sha],
        &["-C", dest_arg, "checkout-index", "-a", "-f", "-u"],
        &["-C", dest_arg, "update-ref", "--no-deref", "HEAD", sha],
    ];
    for step in steps {
        match git(ctx, None, step, false).await? {
            ProcessOutcome::Succeeded => {}
            ProcessOutcome::Cancelled => return Ok(Materialized::Ended(StepResult::Cancelled)),
            ProcessOutcome::Failed => return Ok(Materialized::CheckoutFailed),
        }
    }
    Ok(Materialized::Done)
}

async fn ask(
    store: &StoreWriter,
    ctx: &mut StepContext<'_>,
    request: fn(Option<Token>) -> Request,
) -> Result<bool, ExecError> {
    let token = ctx.github_token.clone().map(Token::new);
    match store.request(request(token)).await {
        Response::Done { notices } => {
            for notice in notices {
                ctx.log(&notice);
            }
            Ok(true)
        }
        Response::Sound => Ok(false),
        Response::Failed { message } => Err(ExecError::Invalid(message)),
        Response::Hello(_) => Err(ExecError::Invalid(
            "store broker protocol error: Hello answered a mirror request".to_owned(),
        )),
    }
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
    if present == ProcessOutcome::Failed {
        ask(store, ctx, |token| Request::FetchMirror { token }).await?;
    }
    for guarded in [&store_root().join("git"), ctx.temp] {
        ensure_disjoint(ctx.workspace, guarded).map_err(invalid)?;
    }
    empty_directory(ctx.workspace).await.map_err(invalid)?;
    let mut state = materialize(ctx, &mirror, facts, &dest, depth).await?;
    if matches!(state, Materialized::CheckoutFailed)
        && ask(store, ctx, |token| Request::RepairMirror { token }).await?
    {
        empty_directory(ctx.workspace).await.map_err(invalid)?;
        state = materialize(ctx, &mirror, facts, &dest, depth).await?;
    }
    match state {
        Materialized::Done => Ok(StepResult::Succeeded),
        Materialized::Ended(ended) => Ok(ended),
        Materialized::CheckoutFailed => Err(ExecError::Invalid(
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
    let cancel = ctx.cancel.clone();
    let work = populate(store, facts, depth, ctx);
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
