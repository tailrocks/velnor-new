//! Staging-only zizmor config and offline zizmor run.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_actionlint::config::{
    ZizmorConfigInput, ZizmorWorkflowText, render_zizmor_yaml,
};
use velnor_actions_mise::{PinnedTool, RuntimePaths, ToolCatalog};
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::validate::{diagnose, is_workflow_path, pinned_output_in_runtime};

/// Staging-only zizmor config at the staging root, never generated.
const ZIZMOR_CONFIG: &str = ".zizmor.yml";

/// Emit the staging-only zizmor config into the staging root.
///
/// Emits a zero-ignore staging config; every emitted ref is hash-pinned,
/// so no `unpinned-uses` location needs an exception. Never touches the
/// generated tree.
pub(crate) fn write_zizmor_config(
    staging: &Path,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    let mut workflows = Vec::new();
    for file in &tree.files {
        if is_workflow_path(&file.path) {
            workflows.push(ZizmorWorkflowText {
                path: file.path.clone(),
                text: file.bytes.clone(),
            });
        }
    }
    let input = ZizmorConfigInput {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        workflows,
    };
    let output = render_zizmor_yaml(&input)?;
    let dest = staging.join(ZIZMOR_CONFIG);
    std::fs::write(&dest, output.yaml)
        .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))
}

/// Run pinned zizmor under a compiled runtime domain.
pub(crate) fn run_zizmor_in_runtime(
    catalog: &ToolCatalog,
    staging: &Path,
    runtime: RuntimePaths,
) -> Result<(), OrchestratorError> {
    let args = [
        "--offline",
        "--no-progress",
        "--color",
        "never",
        "--config",
        ZIZMOR_CONFIG,
        ".",
    ];
    let output = pinned_output_in_runtime(
        catalog,
        "zizmor",
        vec![PinnedTool::Zizmor],
        args.iter().map(OsString::from).collect(),
        staging,
        runtime,
    )?;
    if output.success {
        Ok(())
    } else {
        Err(OrchestratorError::Validation {
            tool: "zizmor".to_owned(),
            problem: diagnose(&output),
        })
    }
}
