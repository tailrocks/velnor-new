//! Fixed Cargo payload argv, sharding, and entry metadata.
//!
//! Pure data helpers over [`TaskGroup`](crate::tasks::TaskGroup); the
//! orchestrator wraps payloads in pinned-tool execution via the Mise adapter.
//! Nextest payloads must run under the Rust plus Nextest pinned toolchain
//! (`mise ... exec rust@<exact> aqua:nextest-rs/nextest/cargo-nextest@<exact>
//! -- cargo <payload>`); a bare `cargo nextest` assumes a preinstalled
//! runner and violates the no-undeclared-preinstalled-tools rule.

use std::ffi::OsString;

use velnor_actions_contract::{ContractError, task_id_for_stack};

use crate::evidence::Evidence;
use crate::profile::NextestProfile;
use crate::tasks::{TaskGroup, TaskKind};

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
/// Cargo-side args (kind, features, target) always precede the `--`
/// separator; only lint args (`-D warnings` for Clippy) follow it, so
/// cargo never forwards feature or target flags to rustc.
/// Unprofiled legacy shape; resolved callers must use [`cargo_payload_with_profile`].
#[must_use]
pub fn cargo_payload_argv(group: &TaskGroup) -> Vec<OsString> {
    let manifest = manifest_for_key(&group.manifest_key);
    let mut args: Vec<OsString> = Vec::new();
    push_kind_args(&mut args, group, &manifest);
    push_feature_args(&mut args, group);
    push_target_arg(&mut args, group);
    push_lint_args(&mut args, group);
    args
}

/// Payload argv with the resolved Nextest profile after `run`; others
/// match [`cargo_payload_argv`] byte for byte (doctests stay separate).
#[must_use]
pub fn cargo_payload_with_profile(group: &TaskGroup, profile: NextestProfile) -> Vec<OsString> {
    let mut argv = cargo_payload_argv(group);
    if group.kind == TaskKind::Nextest {
        let flag = OsString::from("--profile");
        let name = OsString::from(profile.as_str());
        argv.splice(2..2, [flag, name]);
    }
    argv
}

/// Append the per-kind cargo-side payload arguments (never emits `--`).
fn push_kind_args(args: &mut Vec<OsString>, group: &TaskGroup, manifest: &str) {
    let flag = OsString::from;
    match group.kind {
        TaskKind::Fmt => {
            // Workspace fmt groups name no package: their manifest may be
            // virtual, and cargo-fmt reads a bare --manifest-path as a
            // package manifest, exiting "Failed to find targets". --all
            // selects the whole workspace instead. Per-package groups keep
            // the narrow manifest-only form.
            args.push(flag("fmt"));
            if group.package_name.is_empty() {
                args.push(flag("--all"));
            }
            args.extend([flag("--check"), flag("--manifest-path"), flag(manifest)]);
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
///
/// Cargo-side: always before `--` (never forwarded to rustc).
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

/// Append `--target <triple>` for non-host targets (cargo-side: before `--`).
fn push_target_arg(args: &mut Vec<OsString>, group: &TaskGroup) {
    if group.target != "host" {
        args.push(OsString::from("--target"));
        args.push(OsString::from(&group.target));
    }
}

/// Append post-separator lint args (`-- -D warnings` for Clippy, none else).
///
/// This is the only helper allowed to emit `--`; every cargo-side flag
/// (kind, features, target) is pushed before it.
fn push_lint_args(args: &mut Vec<OsString>, group: &TaskGroup) {
    if group.kind == TaskKind::Clippy {
        args.extend([
            OsString::from("--"),
            OsString::from("-D"),
            OsString::from("warnings"),
        ]);
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
