//! Native delivery assembly from reviewed typed policy and compiled tool pins.

#[path = "delivery_pins.rs"]
pub(crate) mod delivery_pins;
#[path = "delivery_tool_context.rs"]
pub(crate) mod delivery_tool_context;
#[path = "oci_delivery.rs"]
pub(crate) mod oci_delivery;

use self::delivery_tool_context::delivery_tool_context;
use self::oci_delivery::{render_oci_release, render_oci_support_files};
use crate::{
    OrchestratorError,
    apt_delivery::{AptRenderContext, AptWorkflowContext, render_apt_delivery},
    prepare::GenerationPreparation,
    release_emit::release_identity::origin_repository,
};
use std::collections::BTreeMap;
use velnor_actions_contract::config::DeliveryConfig;
use velnor_actions_workflow_renderer::RenderedFile;

/// Render configured families; byte-identical shared helpers have one owner.
pub(crate) fn delivery_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let policy = &prep.config.delivery;
    if !active_delivery(policy) {
        return Ok(Vec::new());
    }
    let repository = origin_repository(&prep.root)?;
    let version = env!("CARGO_PKG_VERSION");
    let mut files = Vec::new();
    if let Some(apt) = &policy.apt {
        check_repository(&repository, &apt.consumer_repository, "delivery.apt")?;
        if apt.branch != prep.default_branch {
            return Err(OrchestratorError::config(
                ".velnor/config.toml",
                "delivery.apt.branch",
                "delivery_branch_must_match_default",
            ));
        }
        let context = AptRenderContext {
            workflow: AptWorkflowContext {
                generator_version: version.to_owned(),
                runs_on: prep.runner_label.clone(),
            },
            tools: delivery_tool_context(&prep.config, &prep.runner_label)?,
            buildx_action: delivery_pins::buildx_action(),
            buildx_version: delivery_pins::buildx_version(),
            buildkit_image: delivery_pins::buildkit_image(),
        };
        files.extend(render_apt_delivery(apt, &context)?);
    }
    if let Some(desktop) = &policy.desktop {
        check_desktop_policy(
            desktop.enabled,
            &repository,
            &desktop.repository,
            &prep.default_branch,
        )?;
        if desktop.enabled {
            // Native execution requires sealed tool preparation and bound sources.
            // Keep generation closed until that context is qualified atomically.
            crate::desktop_delivery::require_qualified_profile()?;
        }
    }
    if let Some(oci) = &policy.oci {
        if oci.enabled {
            files.push(render_oci_release(
                oci,
                &delivery_pins::oci_context(prep, repository),
            )?);
            files.extend(render_oci_support_files(version)?);
        }
    }
    unique_support_files(files)
}

/// Inactive families require no origin identity or tool resolution.
fn active_delivery(policy: &DeliveryConfig) -> bool {
    policy.apt.is_some()
        || policy
            .desktop
            .as_ref()
            .is_some_and(|desktop| desktop.enabled)
        || policy.oci.as_ref().is_some_and(|oci| oci.enabled)
}

/// Disabled desktop delivery claims no repository or branch authority.
fn check_desktop_policy(
    enabled: bool,
    actual: &str,
    configured: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    if enabled {
        check_repository(actual, configured, "delivery.desktop")?;
        check_default_branch(branch)?;
    }
    Ok(())
}

/// Desktop schedules bind authority to the validated repository default branch.
fn check_default_branch(branch: &str) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::is_valid_branch_name(branch) {
        return Err(OrchestratorError::config(
            ".velnor/config.toml",
            "delivery.desktop",
            "desktop_default_branch_invalid",
        ));
    }
    Ok(())
}

/// A configured identity cannot authorize another repository's credentials.
fn check_repository(actual: &str, configured: &str, key: &str) -> Result<(), OrchestratorError> {
    if actual != configured {
        return Err(OrchestratorError::config(
            ".velnor/config.toml",
            key,
            "delivery_repository_mismatch",
        ));
    }
    Ok(())
}

/// Only identical fixed shared files may be emitted by multiple families.
pub(crate) fn unique_support_files(
    files: Vec<RenderedFile>,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let mut unique = BTreeMap::<String, RenderedFile>::new();
    for file in files {
        if let Some(previous) = unique.get(&file.path) {
            if previous.bytes != file.bytes {
                return Err(OrchestratorError::Contract {
                    problem: format!("delivery_file_conflict:{}", file.path),
                });
            }
        } else {
            unique.insert(file.path.clone(), file);
        }
    }
    Ok(unique.into_values().collect())
}

#[cfg(test)]
#[path = "delivery_emit_tests.rs"]
mod tests;
