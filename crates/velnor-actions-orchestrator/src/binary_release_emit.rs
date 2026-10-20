//! Manual stable release workflow emission from the shared native inventory.
use crate::{OrchestratorError, prepare::GenerationPreparation};
use std::{fs, path::Path};
use velnor_actions_contract::config::RustBinaryReleaseConfig;
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_mise::GitRequest;
use velnor_actions_workflow_renderer::{
    RenderedFile, WorkflowRoutingPolicy,
    schema2::{
        ConsumerStableReleaseSpec, render_consumer_binary_release, render_release_tap_reconcile,
    },
};

/// Render the generated stable binary release workflow when enabled.
pub(crate) fn files(
    prep: &GenerationPreparation,
    routing: &WorkflowRoutingPolicy,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(rust) = &prep.config.stacks.rust else {
        return Ok(Vec::new());
    };
    let release = &rust.binary_release;
    if !release.stable.enabled {
        return Ok(Vec::new());
    }
    let mut spec =
        crate::binary_preview_emit::specification(prep, release, PackageReleaseChannel::Stable)?;
    spec.stable = Some(ConsumerStableReleaseSpec {
        version_package: release.version_package.clone(),
        release_pr: rust.release.release_pr,
        manifest_path: release.manifest_path.clone(),
        release_plz_argv: crate::stable_release_emit::release_plz_argv(prep)?,
    });
    let mut files = vec![render_consumer_binary_release(&spec, routing)?];
    if spec.homebrew_tap.is_some() {
        files.push(render_release_tap_reconcile(&spec, routing)?);
    }
    Ok(files)
}

pub(crate) fn require_tracked_lockfile(
    prep: &GenerationPreparation,
    config: &RustBinaryReleaseConfig,
) -> Result<(), OrchestratorError> {
    let workspace = Path::new(&config.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| *path != ".")
        .unwrap_or("");
    let relative = if workspace.is_empty() {
        "Cargo.lock".to_owned()
    } else {
        format!("{workspace}/Cargo.lock")
    };
    let lockfile = prep.root.join(&relative);
    match fs::symlink_metadata(&lockfile) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(OrchestratorError::Contract {
                problem: format!("binary_release_lockfile_symlink:{relative}"),
            });
        }
        Ok(_) => {
            return Err(OrchestratorError::Contract {
                problem: format!("binary_release_lockfile_not_regular:{relative}"),
            });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(OrchestratorError::Contract {
                problem: format!("binary_release_lockfile_missing:{relative}"),
            });
        }
        Err(err) => {
            return Err(OrchestratorError::io(
                lockfile.display().to_string(),
                err.to_string(),
            ));
        }
    }
    let request = GitRequest::ls_files(vec![
        "--error-unmatch".into(),
        "--".into(),
        relative.clone().into(),
    ]);
    let output = request.run_in(&prep.root).map_err(contract_error)?;
    if !output.success {
        return Err(OrchestratorError::Contract {
            problem: format!("binary_release_lockfile_untracked:{relative}"),
        });
    }
    Ok(())
}

fn contract_error(problem: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_string(),
    }
}
