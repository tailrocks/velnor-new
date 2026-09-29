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

use crate::evidence::{Evidence, TestRunner};
use crate::tasks::{TaskGroup, TaskKind};

/// Typed Rust task-identity extension (cache §1); unknown schemas disable reuse.
#[derive(Debug, Clone, Serialize)]
pub struct RustTaskIdentityExtension {
    /// Cargo package ID.
    pub package_id: String,
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Execution profile (configuration) name.
    pub profile: String,
    /// Normalized manifest path.
    pub manifest: String,
    /// Workspace/local-package graph digest.
    pub graph_digest: String,
    /// Target kinds and names, sorted.
    pub targets: Vec<String>,
    /// Enabled features, sorted.
    pub features: Vec<String>,
    /// Rust target and profile.
    pub target: String,
    /// Compile driver plus test runner (`driver+runner`).
    pub driver: String,
    /// Cargo config and build-script input digests.
    pub config_digest: String,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<String>,
    /// Rust task kind plus test/archive identity.
    pub kind: String,
    /// Build script reads undeclared inputs; disables reuse and coverage.
    pub undeclared_reads: bool,
    /// `Cargo.lock` digest, when the lockfile is available.
    pub lock_digest: Option<String>,
    /// Archive identity (producing build task id) for Nextest archives only.
    pub archive: Option<String>,
    /// Declared `rerun-if-changed` build inputs, sorted.
    pub rerun_inputs: Vec<String>,
    /// Declared non-Rust task inputs, sorted.
    pub declared_inputs: Vec<String>,
}

/// Inputs for deriving one task-identity extension before selection.
#[derive(Debug, Clone)]
pub struct ExtensionInputs<'a> {
    /// Cargo package ID.
    pub package_id: &'a str,
    /// Workspace identity digest.
    pub workspace_id: &'a str,
    /// Execution profile (configuration) name.
    pub profile: &'a str,
    /// Normalized manifest path.
    pub manifest: &'a str,
    /// Workspace/local-package graph digest.
    pub graph_digest: &'a str,
    /// Target kinds and names.
    pub targets: &'a [String],
    /// Enabled features.
    pub features: &'a [String],
    /// Rust target and profile.
    pub target: &'a str,
    /// Compile driver.
    pub driver: &'a str,
    /// Test runner.
    pub runner: &'a str,
    /// Cargo config and build-script input digests.
    pub config_digest: &'a str,
    /// `Cargo.lock` digest, when the lockfile is available.
    pub lock_digest: Option<&'a str>,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<&'a str>,
    /// Rust task kind.
    pub kind: TaskKind,
    /// Build task id producing the archive (Nextest `Build` only).
    pub archive_source: Option<&'a str>,
    /// Declared `rerun-if-changed` inputs (`None` means unknown).
    pub rerun_inputs: Option<&'a [String]>,
    /// Whether the package carries a build script.
    pub has_build_script: bool,
    /// Declared non-Rust task inputs.
    pub declared_inputs: &'a [String],
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

    /// Derive the extension for one task group before selection and reuse.
    ///
    /// Archives attach only to Nextest `Build` groups (cargo-test never
    /// archives; the archive carries its producing build task id so trust
    /// and retention follow the source). A build script with unknown
    /// `rerun-if-changed` inputs conservatively disables reuse.
    #[must_use]
    pub fn for_task(inputs: &ExtensionInputs<'_>) -> Self {
        let nextest = inputs.runner == TestRunner::CargoNextest.as_str();
        let archive = if inputs.kind == TaskKind::Build && nextest {
            inputs.archive_source.map(str::to_owned)
        } else {
            None
        };
        Self {
            package_id: inputs.package_id.to_owned(),
            workspace_id: inputs.workspace_id.to_owned(),
            profile: inputs.profile.to_owned(),
            manifest: inputs.manifest.to_owned(),
            graph_digest: inputs.graph_digest.to_owned(),
            targets: sorted_unique(inputs.targets),
            features: sorted_unique(inputs.features),
            target: inputs.target.to_owned(),
            driver: format!("{}+{}", inputs.driver, inputs.runner),
            config_digest: inputs.config_digest.to_owned(),
            nextest_digest: inputs.nextest_digest.map(str::to_owned),
            kind: inputs.kind.as_str().to_owned(),
            undeclared_reads: inputs.has_build_script && inputs.rerun_inputs.is_none(),
            lock_digest: inputs.lock_digest.map(str::to_owned),
            archive,
            rerun_inputs: inputs.rerun_inputs.map_or_else(Vec::new, sorted_unique),
            declared_inputs: sorted_unique(inputs.declared_inputs),
        }
    }
}

/// Sorted deduped copy for identity stability.
fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut out = values.to_vec();
    out.sort();
    out.dedup();
    out
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

/// Reject sharded obligations for non-Nextest runners.
///
/// Cargo-test is always one test obligation; only Nextest profiles shard.
///
/// # Errors
///
/// Returns [`ContractError`] when `count` exceeds one for cargo-test.
pub fn require_nextest_for_shards(runner: &str, count: u32) -> Result<(), ContractError> {
    if count > 1 && !shards_allowed(runner) {
        return Err(ContractError::identity(
            "shard_count",
            "cargo_test_single_obligation",
        ));
    }
    Ok(())
}

/// Declared `rerun-if-changed` inputs from build-script output, sorted.
///
/// Accepts both `cargo::rerun-if-changed=PATH` and the legacy
/// `cargo:rerun-if-changed=PATH` forms; anything else is ignored (never an
/// error: unknown directives stay opaque).
#[must_use]
pub fn parse_rerun_changed(output: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        let path = line
            .strip_prefix("cargo::rerun-if-changed=")
            .or_else(|| line.strip_prefix("cargo:rerun-if-changed="))
            .map(str::trim)
            .filter(|path| !path.is_empty());
        if let Some(path) = path {
            paths.push(path.to_owned());
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Adapter entry metadata: detected driver/runner plus evidence ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMetadata {
    /// Detected compile driver.
    pub compile_driver: String,
    /// Detected test runner.
    pub test_runner: String,
    /// Stable evidence ids backing the selection, in evidence order.
    pub evidence_ids: Vec<String>,
}

/// Build entry metadata from one task group plus its profile evidence.
#[must_use]
pub fn entry_metadata(group: &TaskGroup, evidence: &[Evidence]) -> EntryMetadata {
    EntryMetadata {
        compile_driver: group.compile_driver.clone(),
        test_runner: group.test_runner.clone(),
        evidence_ids: evidence.iter().map(evidence_id).collect(),
    }
}

/// Stable id for one evidence sighting: `path:line:digest8`.
#[must_use]
pub fn evidence_id(evidence: &Evidence) -> String {
    let digest = velnor_actions_contract::digest_b3(evidence.command_or_setting.as_bytes());
    let short = digest.get(..8).unwrap_or(&digest);
    format!("{}:{}:{short}", evidence.path, evidence.line)
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
            push_package(args, group);
            args.push(flag("--all-targets"));
            args.extend([flag("--"), flag("-D"), flag("warnings")]);
        }
        TaskKind::Test => {
            args.extend([flag("test"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            push_package(args, group);
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
            push_package(args, group);
            args.extend([flag("--no-tests"), flag("fail")]);
        }
        TaskKind::Doctest => {
            args.extend([flag("test"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            push_package(args, group);
            args.push(flag("--doc"));
        }
        TaskKind::Doc => {
            args.extend([flag("doc"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            push_package(args, group);
            args.push(flag("--no-deps"));
        }
        TaskKind::Build => {
            args.extend([flag("build"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            push_package(args, group);
        }
    }
}

/// Append `--package <name>` unless the group names no package.
fn push_package(args: &mut Vec<OsString>, group: &TaskGroup) {
    if group.package_name.is_empty() {
        return;
    }
    args.push(OsString::from("--package"));
    args.push(OsString::from(&group.package_name));
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
