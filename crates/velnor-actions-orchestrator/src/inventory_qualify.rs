//! Locked/offline qualification shared by fresh inventory consumers.

use std::path::Path;

use velnor_actions_mise::{MetadataQualification, ToolCatalog};

use super::InventoryProvider;
use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};
use crate::generate::ToolSnapshot;

/// Qualify locked/offline resolution where a lockfile pins deps.
/// A tool snapshot brackets the runs, failing closed on tool drift.
/// # Errors
/// Returns `preparation_incomplete` when a lockfile cannot be qualified.
pub(crate) fn qualify_workspaces(
    root: &Path,
    workspaces: &[PlannedWorkspace],
    inventory: InventoryProvider<'_>,
) -> Result<(), OrchestratorError> {
    inventory.require_cargo_allowed(!workspaces.is_empty())?;
    if inventory.validated().is_some() {
        return Ok(());
    }
    let tools = ToolSnapshot::capture(root);
    let catalog = ToolCatalog::pinned();
    for workspace in workspaces {
        let prefix = workspace.record.workspace_root.clone();
        let lock = workspace_lock(&prefix);
        if !root.join(&lock).is_file() {
            continue;
        }
        let manifest = workspace_manifest(&prefix);
        #[cfg(test)]
        super::cargo_probe::record_attempt().map_err(|problem| {
            OrchestratorError::PreparationIncomplete {
                manifest: manifest.clone(),
                problem,
            }
        })?;
        let request = MetadataQualification::new(root.join(&manifest)).map_err(|err| {
            OrchestratorError::PreparationIncomplete {
                manifest: manifest.clone(),
                problem: format!("bad_manifest_path:{err}"),
            }
        })?;
        let qualified = request.command(&catalog).and_then(|command| {
            let output = command.with_cwd(root.to_path_buf()).run()?;
            output.require_success("mise")?;
            output.stdout_text("mise")
        });
        if let Err(err) = qualified {
            let text = err.to_string();
            let first = text.lines().next().unwrap_or("metadata_offline");
            let short: String = first.chars().take(160).collect();
            return Err(OrchestratorError::PreparationIncomplete {
                manifest,
                problem: format!("metadata_offline:{short}"),
            });
        }
    }
    tools.verify(root)?;
    Ok(())
}
