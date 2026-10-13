//! Fixed Cargo payload argv, sharding, and task-identity extension.
//!
//! Pure data helpers over [`TaskGroup`](crate::tasks::TaskGroup); the
//! orchestrator wraps payloads in pinned-tool execution via the Mise adapter.
//! Nextest payloads must run under the Rust plus Nextest pinned toolchain
//! (`mise ... exec rust@<exact> aqua:nextest-rs/nextest/cargo-nextest@<exact>
//! -- cargo <payload>`); a bare `cargo nextest` assumes a preinstalled
//! runner and violates the no-undeclared-preinstalled-tools rule.

use std::ffi::OsString;

use serde::Serialize;
use velnor_actions_contract::{ContractError, task_id_for_stack};

use crate::tasks::{TaskGroup, TaskKind};

/// Typed Rust task-identity extension (cache §1); unknown schemas disable reuse.
#[derive(Debug, Clone, Serialize)]
pub struct RustTaskIdentityExtension {
    /// Cargo package ID.
    pub package_id: String,
    /// Normalized manifest path.
    pub manifest: String,
    /// Workspace/local-package graph digest.
    pub graph_digest: String,
    /// Target kinds and names.
    pub targets: Vec<String>,
    /// Enabled features.
    pub features: Vec<String>,
    /// Rust target and profile.
    pub target: String,
    /// Compile driver and test runner.
    pub driver: String,
    /// Cargo config and build-script input digests.
    pub config_digest: String,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<String>,
    /// Rust task kind plus test/archive identity.
    pub kind: String,
    /// Build script reads undeclared inputs; disables reuse and coverage.
    pub undeclared_reads: bool,
}

impl RustTaskIdentityExtension {
    /// Wrap the extension in the stack-neutral envelope.
    #[must_use]
    pub fn to_stack_extension(&self) -> velnor_actions_contract::StackExtension {
        velnor_actions_contract::StackExtension {
            schema: "rust-task-identity-v1".to_owned(),
            data: serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        }
    }

    /// Reject reuse when build inputs are undeclared or dynamic.
    /// # Errors
    pub fn reuse_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs",
            ));
        }
        Ok(())
    }
}

/// Derive one shard task ID from an unsharded base ID.
/// # Errors
pub fn shard_task_id(base: &str, index: u32, count: u32) -> Result<String, ContractError> {
    let rest = base
        .strip_prefix("stack/")
        .ok_or_else(|| ContractError::identity("task_id", "malformed_task_id"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 4 || rest.contains("shard-") {
        return Err(ContractError::identity("task_id", "bad_base_id"));
    }
    task_id_for_stack(
        parts[0],
        &parts[1..parts.len() - 2].join("/"),
        parts[parts.len() - 2],
        parts[parts.len() - 1],
        Some((index, count)),
    )
}

/// True only for detected Nextest profiles; cargo-test never shards.
#[must_use]
pub fn shards_allowed(test_runner: &str) -> bool {
    test_runner == "cargo_nextest"
}

/// Fixed Cargo payload argv for one task group (kind, features, target).
///
/// Pure data: the orchestrator wraps this payload in a pinned-tool
/// execution via the Mise adapter; this crate builds no invocations.
/// [`TaskKind::Nextest`](crate::tasks::TaskKind::Nextest) payloads require
/// the pinned Nextest tool in that execution, not a preinstalled runner.
#[must_use]
pub fn cargo_payload_argv(group: &TaskGroup) -> Vec<OsString> {
    let manifest = manifest_for_key(&group.manifest_key);
    let mut args: Vec<OsString> = Vec::new();
    push_kind_args(&mut args, group, &manifest);
    push_feature_args(&mut args, group);
    if group.target != "host" {
        args.push(OsString::from("--target"));
        args.push(OsString::from(&group.target));
    }
    args
}

/// Append the per-kind fixed payload arguments.
fn push_kind_args(args: &mut Vec<OsString>, group: &TaskGroup, manifest: &str) {
    let flag = OsString::from;
    match group.kind {
        TaskKind::Fmt => {
            args.extend([
                flag("fmt"),
                flag("--check"),
                flag("--manifest-path"),
                flag(manifest),
            ]);
        }
        TaskKind::Clippy => {
            args.extend([
                flag("clippy"),
                flag("--locked"),
                flag("--offline"),
                flag("--manifest-path"),
                flag(manifest),
            ]);
            if !group.package_name.is_empty() {
                args.extend([flag("--package"), flag(&group.package_name)]);
            }
            args.push(flag("--all-targets"));
        }
        TaskKind::Test => {
            args.extend([flag("test"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            for target_flag in &group.target_flags {
                args.push(flag(target_flag));
            }
        }
        TaskKind::Nextest => {
            // Runs only under the Rust plus Nextest pinned toolchain; the
            // `cargo nextest` subcommand resolves from the pinned tool.
            args.extend([
                flag("nextest"),
                flag("run"),
                flag("--locked"),
                flag("--offline"),
            ]);
            push_manifest(args, manifest);
        }
        TaskKind::Doctest => {
            args.extend([flag("test"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            args.push(flag("--doc"));
        }
        TaskKind::Doc => {
            args.extend([flag("doc"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            args.push(flag("--no-deps"));
        }
        TaskKind::Build => {
            args.extend([flag("build"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
        }
    }
}

/// Append `--manifest-path <manifest>`.
fn push_manifest(args: &mut Vec<OsString>, manifest: &str) {
    args.push(OsString::from("--manifest-path"));
    args.push(OsString::from(manifest));
}

/// Append feature flags unless the group uses default features.
fn push_feature_args(args: &mut Vec<OsString>, group: &TaskGroup) {
    if group.kind == TaskKind::Fmt {
        return;
    }
    if group.features.len() == 1 && group.features[0] == "default" {
        return;
    }
    args.push(OsString::from("--no-default-features"));
    if !group.features.is_empty() {
        args.push(OsString::from("--features"));
        args.push(OsString::from(group.features.join(",")));
    }
}

/// Manifest path for a manifest key.
fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}
