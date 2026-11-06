// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use serde_json::Value;

fn expected_concurrency(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<String, OrchestratorError> {
    use sha2::{Digest as _, Sha256};
    let invalid = || internal("release_pr_workflow_unverified");
    let mut roots = intent.pr_write_paths.iter().filter_map(|path| {
        if path == "Cargo.lock" {
            Some("")
        } else {
            path.strip_suffix("/Cargo.lock")
        }
    });
    let root = roots.next().ok_or_else(invalid)?;
    if roots.next().is_some() {
        return Err(invalid());
    }
    let digest = Sha256::digest(root.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").map_err(|_| invalid())?;
    }
    let lock = velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group(
        &intent.registry,
        &intent.repository,
        &hex,
    )
    .map_err(|_| invalid())?;
    Ok(lock.group)
}

pub(crate) fn verify_reviewed_workflow(
    bytes: &[u8],
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("release_pr_workflow_unverified");
    if bytes.len() > 1024 * 1024 || intent.generator_version != env!("CARGO_PKG_VERSION") {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut options = serde_saphyr::Options::default();
    options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
    let workflow: Value =
        serde_saphyr::from_str_with_options(text, options).map_err(|_| invalid())?;
    let root = workflow.as_object().ok_or_else(invalid)?;
    if root.keys().any(|key| {
        !matches!(
            key.as_str(),
            "name" | "on" | "concurrency" | "permissions" | "cache-mode" | "jobs"
        )
    }) {
        return Err(invalid());
    }
    if workflow["permissions"] != serde_json::json!({"contents": "read"})
        || workflow["cache-mode"] != "read"
    {
        return Err(invalid());
    }
    let expected = expected_concurrency(intent)?;
    if workflow["concurrency"]
        != serde_json::json!({"group": expected, "cancel-in-progress": false})
    {
        return Err(invalid());
    }
    verify_reviewed_jobs(&workflow, intent)
}

fn verify_reviewed_jobs(
    workflow: &Value,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), OrchestratorError> {
    use velnor_actions_contract::config::RustReleaseConfig;
    use velnor_actions_mise::{MISE_VERSION, PinnedTool, ToolCatalog};
    use velnor_actions_workflow_renderer::{MiseSetup, setup::MISE_BINARY_SHA256_LINUX_X64};
    let invalid = || internal("release_pr_workflow_unverified");
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, &intent.rust_version)
        .map_err(|_| invalid())?
        .with_export_rustup_toolchain(false);
    if catalog.version(PinnedTool::ReleasePlz) != intent.release_plz_version {
        return Err(invalid());
    }
    let release = RustReleaseConfig {
        release_pr: true,
        manifest_path: intent.manifest_path.clone(),
        environment: intent.environment.clone(),
        authentication: Some(intent.authentication),
        ..RustReleaseConfig::default()
    };
    let mise = MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
        &intent.generator_version,
        &intent.runner,
        &intent.generator_source_sha,
        &intent.generator_manifest_sha256,
    )
    .map_err(|_| invalid())?;
    let preparation_gate = format!(
        "github.repository == '{}' && github.event_name == 'push' && github.ref == 'refs/heads/{}' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'",
        intent.repository, intent.default_branch
    );
    let jobs =
        crate::release::release_steps::assemble_jobs(&crate::release::release_steps::JobInputs {
            release: &release,
            gate: String::new(),
            preparation_gate,
            registry: None,
            label: &intent.runner,
            mise: &mise,
            catalog: &catalog,
            acquire: &acquire,
            generator_version: &intent.generator_version,
            generator_source_sha: &intent.generator_source_sha,
            generator_manifest_sha256: &intent.generator_manifest_sha256,
        })?;
    for id in [
        "release-preflight",
        "release-preparation",
        "release-pr-create",
    ] {
        let expected_yaml = velnor_actions_workflow_renderer::release_tree::render_release_job(
            jobs.get(id).ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let expected: Value = serde_saphyr::from_str(&expected_yaml).map_err(|_| invalid())?;
        if workflow["jobs"][id] != expected {
            return Err(invalid());
        }
    }
    Ok(())
}
