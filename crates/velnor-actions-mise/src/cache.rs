// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, validate_digest};

use crate::error::MiseError;

pub const TASK_ARTIFACTS_DIR_NAME: &str = "task-artifacts";
pub const TASK_ARTIFACTS_VERSION: &str = "v2";
pub const TASK_CACHE_DIR_ENV: &str = "MISE_TASK_CACHE_DIR";
pub const CACHE_DIR_ENV: &str = "MISE_CACHE_DIR";
pub const TASK_CACHE_MODE_ENV: &str = "MISE_TASK_CACHE";
pub const TASK_DEF_PREFIX: &str = "$RUNNER_TEMP/velnor/tasks/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskCacheMode {
    ReadWrite,
    ReadOnly,
    WriteOnly,
    Off,
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

#[must_use]
pub fn task_artifact_dir_from_env() -> Option<PathBuf> {
    let task_cache_dir = std::env::var_os(TASK_CACHE_DIR_ENV).map(PathBuf::from);
    let cache_dir = std::env::var_os(CACHE_DIR_ENV).map(PathBuf::from);
    resolve_task_artifact_dir(task_cache_dir.as_deref(), cache_dir.as_deref())
}

pub fn cache_mode_from_env() -> Result<Option<TaskCacheMode>, MiseError> {
    let raw = std::env::var_os(TASK_CACHE_MODE_ENV).unwrap_or_default();
    if raw.is_empty() {
        return Ok(None);
    }
    TaskCacheMode::from_str(&raw.to_string_lossy()).map(Some)
}

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

fn escapes_root(relative: &str) -> MiseError {
    MiseError::ArtifactEscapesRoot {
        path: relative.to_owned(),
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedTaskDescriptor {
    pub task_name: String,
    pub sources: Vec<String>,
    pub outputs: Vec<String>,
    pub command_inputs: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub tools: Vec<String>,
    pub dep_keys: Vec<String>,
}

impl CachedTaskDescriptor {
    pub fn validate(&self) -> Result<(), MiseError> {
        if self.sources.is_empty() {
            return Err(ineligible(&self.task_name, "task_declares_no_sources"));
        }
        Ok(())
    }

    pub fn cache_inputs_digest(&self) -> Result<String, MiseError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self).map_err(|err| MiseError::Contract {
            problem: err.to_string(),
        })?;
        Ok(digest_b3(&bytes))
    }
}

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

pub fn validate_task_def_path(path: &str) -> Result<(), MiseError> {
    if !path.starts_with(TASK_DEF_PREFIX) || path.strip_suffix(".toml").is_none() {
        return Err(escapes_root(path));
    }
    if path.contains("..") || path.contains(".mise/tasks") {
        return Err(escapes_root(path));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedTaskDef {
    pub name: String,
    pub run: Vec<String>,
    pub sources: Vec<String>,
    pub outputs: Option<Vec<String>>,
    pub command_inputs: Vec<String>,
}

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

fn toml_strings(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|value| format!("{value:?}")).collect();
    format!("[{}]", items.join(", "))
}

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

fn ineligible(task: &str, reason: &str) -> MiseError {
    MiseError::CacheNotEligible {
        task: task.to_owned(),
        reason: reason.to_owned(),
    }
}

#[must_use]
pub fn save_allowed(layer_trust: &str, event: &str, passed: bool) -> bool {
    passed && event == "push" && matches!(layer_trust, "trusted" | "pr")
}
