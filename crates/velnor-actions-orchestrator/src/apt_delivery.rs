//! Fixed APT delivery adapter: typed policy becomes workflows and native helpers.

use velnor_actions_contract::{canonical_json_str, config::AptDeliveryConfig};

use velnor_actions_workflow_renderer::{
    RenderError, WorkflowDocumentContext, marker, render::RenderedFile, render_workflow_document,
};

#[path = "apt_delivery_workflow.rs"]
mod workflow;

#[path = "apt_delivery_context.rs"]
mod context;
pub use context::{AptRenderContext, AptWorkflowContext};

/// Generated APT delivery workflow, distinct from the Cargo release family.
pub const APT_DELIVERY_WORKFLOW_PATH: &str = ".github/workflows/delivery-apt.yml";

/// Complete fixed APT family ownership, including the shared admission helper.
pub const APT_DELIVERY_TREE_PATHS: &[&str] = &[
    APT_DELIVERY_WORKFLOW_PATH,
    ".github/velnor/apt-delivery.jsonc",
    ".github/velnor/apt_delivery.py",
    ".github/velnor/delivery_apt_transport.py",
    ".github/velnor/apt_verify.sh",
    ".github/velnor/apt_stage.sh",
    ".github/velnor/apt_transport_incoming.sh",
    ".github/velnor/native_pages_admission.sh",
    ".github/velnor/apt_result.sh",
    ".github/velnor/release_admission.py",
    ".github/velnor/release_admission_default_branch.sh",
];

/// Former generated companions removed only when their canonical marker proves ownership.
pub const APT_RETIRED_TREE_PATHS: &[&str] = &[
    ".github/velnor/delivery_apt_core.py",
    ".github/velnor/delivery_apt_verify.py",
    ".github/velnor/delivery_apt_stage.py",
    ".github/velnor/delivery_apt_stage_feed.py",
    ".github/velnor/delivery_apt_stage_publish.py",
];

/// Render the closed APT delivery adapter and its fixed helper files.
///
/// # Errors
/// Returns an error for invalid typed policy or render context.
pub fn render_apt_delivery(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
) -> Result<Vec<RenderedFile>, RenderError> {
    config
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    context.validate()?;
    let document = workflow::workflow(config, context)?;
    let renderer = WorkflowDocumentContext {
        generator_version: context.workflow.generator_version.clone(),
        source_helpers: document.source_helpers,
        native_pages_approvals: document.pages_approvals,
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let mut files = vec![RenderedFile {
        path: APT_DELIVERY_WORKFLOW_PATH.to_owned(),
        bytes: render_workflow_document(&document.ir, &renderer)?,
    }];
    files.extend(
        velnor_actions_workflow_renderer::source_helper::source_helper_files(
            &renderer.source_helpers,
            &renderer.generator_version,
        )?,
    );
    let json = canonical_json_str(config).map_err(RenderError::Contract)?;
    files.push(generated(
        ".github/velnor/apt-delivery.jsonc",
        &format!("{json}\n"),
        &context.workflow.generator_version,
    )?);
    let sources = velnor_actions_native::apt::support_sources(&context.workflow.generator_version)
        .map_err(RenderError::Contract)?;
    files.extend(sources.files().iter().map(|file| RenderedFile {
        path: file.path().to_owned(),
        bytes: file.source().to_owned(),
    }));
    files.push(generated(
        crate::release_emit::release_admission::ADMISSION_PATH,
        crate::release_emit::release_admission::source(),
        &context.workflow.generator_version,
    )?);
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn generated(path: &str, body: &str, version: &str) -> Result<RenderedFile, RenderError> {
    Ok(RenderedFile {
        path: path.to_owned(),
        bytes: marker::with_marker(version, body)?,
    })
}

#[cfg(test)]
#[path = "apt_delivery_tests.rs"]
mod tests;
