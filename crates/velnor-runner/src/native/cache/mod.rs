// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

mod archive;
mod descent;
mod guard;
mod inputs;
mod nest;
mod place;
mod restore;
mod scope;
mod stamp;
mod store;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};

use crate::broker::cache::{CacheOwner, sha256_hex};
use crate::broker::{Request, Response, StoreWriter};
use crate::exec::{
    DeclaredInputs, ExecError, JobCancel, StepAdapter, StepContext, StepFuture, StepResult,
};
use crate::native::Umask;
use crate::store::store_root;
use inputs::{CacheInputs, CacheKind, invalid};
use scope::{CacheAccess, CacheMode};

struct Plan {
    root: PathBuf,
    owner: CacheOwner,
    access: CacheAccess,
    roots: Vec<PathBuf>,
    umask: Umask,
    version: String,
    inputs: CacheInputs,
}

#[derive(Default)]
struct Done {
    lines: Vec<String>,
    outputs: Vec<(&'static str, String)>,
    hit: Option<store::CacheEntry>,
}

fn resolve(path: &str, workspace: &Path, home: Option<&str>) -> Result<PathBuf, ExecError> {
    let expanded = match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = home.ok_or_else(|| invalid("cannot expand '~' without HOME"))?;
            PathBuf::from(format!("{home}{rest}"))
        }
        _ => PathBuf::from(path),
    };
    Ok(archive::normalize(&workspace.join(expanded)))
}

fn portable(path: &str, temp: &Path, workspace: &Path) -> String {
    for (base, marker) in [(temp, "\0runner.temp"), (workspace, "\0workspace")] {
        if let Ok(rest) = Path::new(path).strip_prefix(base) {
            return format!("{marker}/{}", rest.display());
        }
    }
    path.to_owned()
}

impl Plan {
    fn new(
        root: PathBuf,
        owner: CacheOwner,
        umask: Umask,
        ctx: &StepContext<'_>,
        inputs: CacheInputs,
    ) -> Result<Self, ExecError> {
        let home = ctx
            .env
            .get("HOME")
            .cloned()
            .or_else(|| std::env::var("HOME").ok());
        let roots = inputs
            .paths
            .iter()
            .map(|path| resolve(path, ctx.workspace, home.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;
        let declared: Vec<String> = inputs
            .paths
            .iter()
            .map(|path| portable(path, ctx.temp, ctx.workspace))
            .collect();
        Ok(Self {
            root,
            owner,
            access: scope::cache_access(ctx.job)?,
            roots,
            umask,
            version: sha256_hex(&declared.join("\n")),
            inputs,
        })
    }
}

fn read_denied(mode: CacheMode, inputs: &CacheInputs) -> Done {
    Done {
        lines: vec![
            format!("::warning::Failed to restore: cache read denied: cache-mode is {mode}"),
            missed(inputs),
        ],
        outputs: Vec::new(),
        hit: None,
    }
}

fn missed(inputs: &CacheInputs) -> String {
    let keys = std::iter::once(&inputs.key)
        .chain(&inputs.restore_keys)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    format!("Cache not found for input keys: {keys}")
}

fn restore(plan: &Plan, cancel: Option<&JobCancel>) -> anyhow::Result<Done> {
    let inputs = &plan.inputs;
    let Some(hit) = store::find(
        &plan.root,
        &plan.owner,
        &plan.access.rules,
        &inputs.key,
        &inputs.restore_keys,
        &plan.version,
        cancel,
    )?
    else {
        return Ok(Done {
            lines: vec![missed(inputs)],
            ..Done::default()
        });
    };
    restore_hit(plan, hit, cancel)
}

fn restore_hit(
    plan: &Plan,
    hit: store::CacheEntry,
    cancel: Option<&JobCancel>,
) -> anyhow::Result<Done> {
    let inputs = &plan.inputs;
    let mut done = Done::default();
    if inputs.lookup_only {
        done.lines.push(format!(
            "Cache found and can be restored from key: {}",
            hit.key
        ));
    } else {
        let members = match restore::unpack(&hit.data(), &plan.roots, plan.umask, cancel) {
            Err(error) if error.is::<restore::Vanished>() => {
                done.lines.push(format!(
                    "::warning::Failed to restore: the cache entry for key {} vanished before it was opened",
                    hit.key
                ));
                done.lines.push(missed(inputs));
                return Ok(done);
            }
            other => other.with_context(|| format!("restore cache key {}", hit.key))?,
        };
        if members == 0 {
            done.lines.push(format!(
                "::warning::Failed to restore: the cache entry for key {} holds no members",
                hit.key
            ));
            done.lines.push(missed(inputs));
            return Ok(done);
        }
        done.lines
            .push(format!("Cache restored from key: {}", hit.key));
    }
    let exact = hit.key.eq_ignore_ascii_case(&inputs.key);
    done.outputs.push(("cache-hit", exact.to_string()));
    done.outputs.push(("cache-matched-key", hit.key.clone()));
    done.hit = Some(hit);
    Ok(done)
}

fn saving(message: &str) -> String {
    format!("::warning::Failed to save: {message}")
}

async fn publish(
    store: &StoreWriter,
    plan: &Plan,
    cancel: Option<JobCancel>,
) -> anyhow::Result<String> {
    let key = &plan.inputs.key;
    let mut upload = match store.begin_cache(key, &plan.version).await {
        Ok(upload) => upload,
        Err(Response::Denied(reason)) => {
            return Ok(saving(&format!("cache write denied: {reason}")));
        }
        Err(Response::Failed { message }) => return Ok(saving(&message)),
        Err(other) => bail!("the store broker answered a cache upload with {other:?}"),
    };
    let writer = upload.take_writer().context("the cache pipe is missing")?;
    let roots = plan.roots.clone();
    let watch = cancel.clone();
    let packed =
        tokio::task::spawn_blocking(move || archive::pack(&roots, writer, cancel.as_ref()))
            .await
            .context("pack task failed")?;
    let bytes = match packed {
        Ok(bytes) => bytes,
        Err(error) if error.is::<archive::Cancelled>() => {
            upload.abort().await;
            return Err(error);
        }
        Err(error) => {
            return Ok(match upload.abort().await {
                Response::Failed { message } => saving(&message),
                _ => saving(&format!("{error:#}")),
            });
        }
    };
    let answer = match &watch {
        Some(cancel) => tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(archive::Cancelled.into()),
            answer = upload.seal(bytes) => answer,
        },
        None => upload.seal(bytes).await,
    };
    Ok(match answer {
        Response::Published { .. } => format!("Cache saved with key: {key}"),
        Response::AlreadyExists => format!("Cache already exists for key {key}"),
        Response::Denied(reason) => saving(&format!("cache write denied: {reason}")),
        Response::Failed { message } => saving(&message),
        other => bail!("the store broker answered a cache seal with {other:?}"),
    })
}

async fn save(store: &StoreWriter, plan: &Plan, cancel: Option<JobCancel>) -> anyhow::Result<Done> {
    let key = &plan.inputs.key;
    let line = if !plan.access.mode.can_write() {
        saving(&format!(
            "cache write denied: cache-mode is {}",
            plan.access.mode
        ))
    } else if let Some(denied) = store
        .facts()
        .ok()
        .and_then(|facts| facts.cache_write_scope().err())
    {
        saving(&format!("cache write denied: {denied}"))
    } else if !plan
        .roots
        .iter()
        .any(|root| fs::symlink_metadata(root).is_ok())
    {
        "[warning]Path Validation Error: Path(s) specified in the action for caching do(es) not exist, hence no cache is being saved.".to_owned()
    } else if store::exists(
        &plan.root,
        &plan.owner,
        &plan.access.rules.write,
        &plan.version,
        key,
    ) {
        format!("Cache already exists for key {key}")
    } else {
        publish(store, plan, cancel).await?
    };
    Ok(Done {
        lines: vec![line],
        outputs: Vec::new(),
        hit: None,
    })
}

fn failed_to_save(
    kind: CacheKind,
    ctx: &mut StepContext<'_>,
    error: ExecError,
) -> Result<StepResult, ExecError> {
    match error {
        ExecError::Invalid(message) if kind == CacheKind::Save => {
            ctx.log(&format!("[warning]{message}"));
            Ok(StepResult::Succeeded)
        }
        other => Err(other),
    }
}

async fn cache_step(
    store: &StoreWriter,
    umask: Umask,
    ctx: &mut StepContext<'_>,
) -> Result<StepResult, ExecError> {
    let kind = CacheKind::of(&ctx.reference)
        .ok_or_else(|| invalid(format!("{} is not a cache action", ctx.reference)))?;
    if let Err(error) = inputs::check_revision(&ctx.reference) {
        return failed_to_save(kind, ctx, error);
    }
    let inputs = match CacheInputs::parse(&ctx.inputs, kind) {
        Ok(inputs) => inputs,
        Err(error) => return failed_to_save(kind, ctx, error),
    };
    if kind == CacheKind::Restore {
        ctx.set_output("cache-primary-key", inputs.key.clone());
    }
    if kind == CacheKind::Restore {
        let mode = scope::mode_of(ctx.job)?;
        if !mode.can_read() {
            let done = read_denied(mode, &inputs);
            for line in &done.lines {
                ctx.log(line);
            }
            return Ok(StepResult::Succeeded);
        }
    }
    let owner = match store.facts() {
        Ok(facts) => CacheOwner::of(facts),
        Err(reason) if kind == CacheKind::Save => {
            ctx.log(&saving(&reason));
            return Ok(StepResult::Succeeded);
        }
        Err(reason) => return Err(invalid(reason)),
    };
    let plan = match Plan::new(store_root(), owner, umask, ctx, inputs) {
        Ok(plan) => plan,
        Err(error) => return failed_to_save(kind, ctx, error),
    };
    let cancel = ctx.cancel.clone();
    let outcome = match kind {
        CacheKind::Restore => {
            let cancel = cancel.clone();
            tokio::task::spawn_blocking(move || restore(&plan, cancel.as_ref()))
                .await
                .map_err(|error| anyhow!("cache task failed: {error}"))
                .and_then(|restored| restored)
        }
        CacheKind::Save => save(store, &plan, cancel).await,
    };
    let done = match outcome {
        Ok(done) => done,
        Err(error) if error.is::<archive::Cancelled>() => return Ok(StepResult::Cancelled),
        Err(error) => return Err(invalid(format!("{error:#}"))),
    };
    for line in &done.lines {
        ctx.log(line);
    }
    if let Some(hit) = &done.hit {
        let request = Request::TouchCache {
            scope: hit.scope.0.clone(),
            key: hit.key.clone(),
            version: hit.version.clone(),
        };
        if let Response::Failed { message } = store.request(request).await {
            ctx.log(&format!(
                "::warning::Failed to record the access of cache key {}: {message}",
                hit.key
            ));
        }
    }
    for (name, value) in done.outputs {
        ctx.set_output(name, value);
    }
    Ok(StepResult::Succeeded)
}

pub(crate) struct CacheAdapter {
    store: Arc<StoreWriter>,
    umask: Umask,
}

impl CacheAdapter {
    pub(crate) fn new(store: Arc<StoreWriter>, umask: Umask) -> Self {
        Self { store, umask }
    }
}

impl StepAdapter for CacheAdapter {
    fn handles(&self, reference: &str) -> bool {
        CacheKind::of(reference).is_some()
    }

    fn problem(&self, reference: &str, inputs: &DeclaredInputs) -> Option<String> {
        inputs::declared_problem(CacheKind::of(reference)?, reference, inputs)
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        Box::pin(cache_step(&self.store, self.umask, ctx))
    }
}
