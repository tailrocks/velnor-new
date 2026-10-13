//! Read-only task-result cache helpers per the qualified mise schema.
//!
//! Covers modes, the `task-artifacts/v2` layout, artifact resolution, digest
//! verification, sources allowlist, task defs, reuse checks, and the exact-base
//! `gh` lookup. Nothing here writes artifacts or configures remote caching.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, validate_digest};

use crate::error::MiseError;

/// Directory holding cached task outputs inside a mise cache parent.
pub const TASK_ARTIFACTS_DIR_NAME: &str = "task-artifacts";
/// Artifact layout version kept as the final path segment.
pub const TASK_ARTIFACTS_VERSION: &str = "v2";
/// Env var overriding the task-cache parent directory.
pub const TASK_CACHE_DIR_ENV: &str = "MISE_TASK_CACHE_DIR";
/// Env var holding the default mise cache directory.
pub const CACHE_DIR_ENV: &str = "MISE_CACHE_DIR";
/// Env var selecting the task-cache access mode.
pub const TASK_CACHE_MODE_ENV: &str = "MISE_TASK_CACHE";
/// Runner-temp prefix owning every generated task definition.
pub const TASK_DEF_PREFIX: &str = "$RUNNER_TEMP/velnor/tasks/";

/// Qualified task-cache access modes (`--task-cache` / `MISE_TASK_CACHE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskCacheMode {
    /// Read and write cached task results.
    ReadWrite,
    /// Read cached task results without writing.
    ReadOnly,
    /// Write cached task results without reading.
    WriteOnly,
    /// Disable task-result caching.
    Off,
    /// Cache to the local directory only, never remote.
    LocalOnly,
}

impl Display for TaskCacheMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(match self {
            Self::ReadWrite => "read-write",
            Self::ReadOnly => "read-only",
            Self::WriteOnly => "write-only",
            Self::Off => "off",
            Self::LocalOnly => "local-only",
        })
    }
}

impl FromStr for TaskCacheMode {
    type Err = MiseError;

    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "read-write" => Ok(Self::ReadWrite),
            "read-only" => Ok(Self::ReadOnly),
            "write-only" => Ok(Self::WriteOnly),
            "off" => Ok(Self::Off),
            "local-only" => Ok(Self::LocalOnly),
            _ => Err(MiseError::UnknownCacheMode {
                mode: mode.to_owned(),
            }),
        }
    }
}

/// Resolve the artifact dir from explicit parents (`task-artifacts/v2` suffix).
#[must_use]
pub fn resolve_task_artifact_dir(
    task_cache_dir: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Option<PathBuf> {
    let parent = task_cache_dir.or(cache_dir)?;
    Some(
        parent
            .join(TASK_ARTIFACTS_DIR_NAME)
            .join(TASK_ARTIFACTS_VERSION),
    )
}

/// Resolve the artifact dir from the process environment.
#[must_use]
pub fn task_artifact_dir_from_env() -> Option<PathBuf> {
    let task_cache_dir = std::env::var_os(TASK_CACHE_DIR_ENV).map(PathBuf::from);
    let cache_dir = std::env::var_os(CACHE_DIR_ENV).map(PathBuf::from);
    resolve_task_artifact_dir(task_cache_dir.as_deref(), cache_dir.as_deref())
}

/// Read the task-cache mode from [`TASK_CACHE_MODE_ENV`].
/// # Errors
/// Returns [`MiseError::UnknownCacheMode`] for unqualified values.
pub fn cache_mode_from_env() -> Result<Option<TaskCacheMode>, MiseError> {
    let raw = std::env::var_os(TASK_CACHE_MODE_ENV).unwrap_or_default();
    if raw.is_empty() {
        return Ok(None);
    }
    TaskCacheMode::from_str(&raw.to_string_lossy()).map(Some)
}

/// Join a relative artifact name onto its root, refusing escapes.
/// # Errors
/// Returns [`MiseError::ArtifactEscapesRoot`] for absolute, empty, or escaping names.
pub fn artifact_path(root: &Path, relative: &str) -> Result<PathBuf, MiseError> {
    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return Err(escapes_root(relative));
    }
    let mut joined = PathBuf::from(root);
    let mut named = false;
    for component in candidate.components() {
        match component {
            Component::Normal(part) => {
                joined.push(part);
                named = true;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(escapes_root(relative));
            }
        }
    }
    if named {
        Ok(joined)
    } else {
        Err(escapes_root(relative))
    }
}

/// Build the shared rejection for an artifact name outside its root.
fn escapes_root(relative: &str) -> MiseError {
    MiseError::ArtifactEscapesRoot {
        path: relative.to_owned(),
    }
}

/// Read one artifact file into memory.
/// # Errors
/// Returns not-found/unreadable errors for missing or failing reads.
pub fn read_artifact_bytes(path: &Path) -> Result<Vec<u8>, MiseError> {
    std::fs::read(path).map_err(|err| {
        let path = path.to_string_lossy().into_owned();
        if err.kind() == ErrorKind::NotFound {
            MiseError::ArtifactNotFound { path }
        } else {
            MiseError::ArtifactUnreadable {
                path,
                message: err.to_string(),
            }
        }
    })
}

/// Verify artifact bytes against an expected `b3-` digest.
/// # Errors
/// Returns [`MiseError::InvalidDigest`] or [`MiseError::DigestMismatch`].
pub fn verify_artifact_digest(bytes: &[u8], expected: &str) -> Result<(), MiseError> {
    validate_digest(expected).map_err(|err| MiseError::InvalidDigest {
        value: expected.to_owned(),
        problem: err.to_string(),
    })?;
    let actual = digest_b3(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(MiseError::DigestMismatch {
            expected: expected.to_owned(),
            actual,
        })
    }
}

/// Declared cache-key inputs of one task, per the qualified schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedTaskDescriptor {
    /// Task name as addressed by `mise run`.
    pub task_name: String,
    /// Source globs feeding the key; at least one is required.
    pub sources: Vec<String>,
    /// Declared outputs restored from a cache hit.
    pub outputs: Vec<String>,
    /// Command inputs hashed with captured stdout and stderr.
    pub command_inputs: Vec<String>,
    /// Declared environment entries (`cache.env`) feeding the key.
    pub env: BTreeMap<String, String>,
    /// Resolved `<tool>@<exact>` selectors feeding the key.
    pub tools: Vec<String>,
    /// Dependency artifact keys feeding the key.
    pub dep_keys: Vec<String>,
}

impl CachedTaskDescriptor {
    /// Reject descriptors mise could never cache (currently: no sources).
    /// # Errors
    /// Returns [`MiseError::CacheNotEligible`] when `sources` is empty.
    pub fn validate(&self) -> Result<(), MiseError> {
        if self.sources.is_empty() {
            return Err(ineligible(&self.task_name, "task_declares_no_sources"));
        }
        Ok(())
    }

    /// Deterministic digest over the declared inputs (canonical JSON + BLAKE3).
    /// # Errors
    /// Returns [`MiseError::Contract`] when canonical serialization fails.
    pub fn cache_inputs_digest(&self) -> Result<String, MiseError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self).map_err(|err| MiseError::Contract {
            problem: err.to_string(),
        })?;
        Ok(digest_b3(&bytes))
    }
}

/// Reject a sources-archive path outside `registry/` or `git/`.
/// # Errors
pub fn validate_sources_path(path: &str) -> Result<(), MiseError> {
    let mut parts = path.split('/');
    let top = parts.next().unwrap_or("");
    if !matches!(top, "registry" | "git") || path.starts_with('/') || path.contains('\\') {
        return Err(escapes_root(path));
    }
    if parts.any(|seg| seg == ".." || seg.starts_with("credentials")) {
        return Err(escapes_root(path));
    }
    Ok(())
}

/// Task-cache mode for one workflow event: local/pr/fork/merge/push/release.
/// # Errors
pub fn mode_for_event(event: &str) -> Result<TaskCacheMode, MiseError> {
    match event {
        "local" => Ok(TaskCacheMode::LocalOnly),
        "pull_request" | "merge_group" | "fork" => Ok(TaskCacheMode::ReadOnly),
        "push" => Ok(TaskCacheMode::ReadWrite),
        "release" | "qualification" => Ok(TaskCacheMode::Off),
        _ => Err(MiseError::UnknownCacheMode {
            mode: event.to_owned(),
        }),
    }
}

/// Reject generated task files outside runner temp (never `.mise/tasks`).
/// # Errors
pub fn validate_task_def_path(path: &str) -> Result<(), MiseError> {
    if !path.starts_with(TASK_DEF_PREFIX) || path.strip_suffix(".toml").is_none() {
        return Err(escapes_root(path));
    }
    if path.contains("..") || path.contains(".mise/tasks") {
        return Err(escapes_root(path));
    }
    Ok(())
}

/// One qualified deterministic task definition (Gate 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedTaskDef {
    /// Task name addressed by `mise run`.
    pub name: String,
    /// Fixed command arguments.
    pub run: Vec<String>,
    /// Source globs; at least one is required.
    pub sources: Vec<String>,
    /// Declared outputs (`None` renders `outputs = []`).
    pub outputs: Option<Vec<String>>,
    /// Command inputs hashed with captured streams.
    pub command_inputs: Vec<String>,
}

/// Render a versioned task TOML: marker first, then fixed fields.
///
/// Crate-private: task-cache TOML is a schema change enabled only with
/// Gate-6 fixtures (see `gate6`), so external callers must go through the
/// gated wrapper.
/// # Errors
pub(crate) fn render_task_toml(version: &str, def: &QualifiedTaskDef) -> Result<String, MiseError> {
    if def.sources.is_empty() || def.run.is_empty() || def.name.trim().is_empty() {
        return Err(ineligible(&def.name, "task_def_incomplete"));
    }
    let outputs = match &def.outputs {
        Some(outputs) => toml_strings(outputs),
        None => "[]".to_owned(),
    };
    Ok([
        format!("# velnor-actions {version}\n"),
        format!("run = {}\n", toml_strings(&def.run)),
        format!("sources = {}\n", toml_strings(&def.sources)),
        format!("outputs = {outputs}\n"),
        "[cache]\n".to_owned(),
        format!("command_inputs = {}\n", toml_strings(&def.command_inputs)),
    ]
    .join(""))
}

/// Quote strings as a TOML array of double-quoted values.
fn toml_strings(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|value| format!("{value:?}")).collect();
    format!("[{}]", items.join(", "))
}

/// Fixed `mise run --task-cache <mode> <task> --file <path>` argv.
///
/// UNQUALIFIED (P04-10): pinned mise 2026.10.4 has no `mise run
/// --file` flag (probe 2026-10-08), so this shape cannot execute as
/// rendered; modes, `--no-*` flags, and `task-artifacts/v2` verified.
/// # Errors
pub fn task_run_argv(
    mode: TaskCacheMode,
    task: &str,
    file: &str,
) -> Result<Vec<String>, MiseError> {
    validate_task_def_path(file)?;
    if !velnor_actions_contract::is_valid_mise_task_name(task) {
        return Err(ineligible(task, "bad_task_name"));
    }
    Ok([
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "run",
        "--task-cache",
        &mode.to_string(),
        task,
        "--file",
        file,
    ]
    .iter()
    .map(ToString::to_string)
    .collect())
}

/// Reject reuse for nondeterministic or undeclared-state task kinds.
/// # Errors
pub fn qualify_reuse(
    kind: &str,
    network: bool,
    clock: bool,
    random: bool,
) -> Result<(), MiseError> {
    if matches!(kind, "publish" | "deploy" | "notify" | "service") || network || clock || random {
        return Err(ineligible(kind, "task_not_eligible"));
    }
    Ok(())
}

/// Verify every declared output is present with a matching digest.
///
/// Zero-byte observations fail under the single P04 rule in
/// [`crate::restore_evidence::output_bytes_complete`], shared with the
/// orchestrator layer so both layers always agree.
/// # Errors
pub fn verify_reused_outputs(
    task: &str,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
) -> Result<(), MiseError> {
    for path in declared {
        let Some((_, bytes, digest)) = observed.iter().find(|(name, _, _)| name == path) else {
            return Err(MiseError::ArtifactNotFound { path: path.clone() });
        };
        if !crate::restore_evidence::output_bytes_complete(bytes) {
            return Err(ineligible(task, "task_result_incomplete"));
        }
        verify_artifact_digest(bytes, digest)
            .map_err(|_| ineligible(task, "task_result_incomplete"))?;
    }
    Ok(())
}

/// Shared rejection for an ineligible cache task.
fn ineligible(task: &str, reason: &str) -> MiseError {
    MiseError::CacheNotEligible {
        task: task.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Save allowlist: producer-successful pushes only, for every layer.
///
/// A save needs its producer to have passed and a protected-push event;
/// failed runs never save, and PR, fork, merge-group, release, local,
/// and unknown events never save through this path (PR task caches are
/// read-only; release caching is off). Unknown trust scopes deny closed.
#[must_use]
pub fn save_allowed(layer_trust: &str, event: &str, passed: bool) -> bool {
    passed && event == "push" && matches!(layer_trust, "trusted" | "pr")
}
