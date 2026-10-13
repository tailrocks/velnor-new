//! Fixed runner-temp path validators for the plan-job closure.

use velnor_actions_workflow_steps::RenderError;

/// Helper binaries live under the staged runner-temp dir, never the repo.
pub(crate) fn validate_helper_path(path: &str) -> Result<(), RenderError> {
    let ok = path.starts_with("$RUNNER_TEMP/velnor/")
        && !path.contains(' ')
        && !path.contains('\n')
        && !path.split('/').any(|seg| seg.is_empty() || seg == "..")
        && !path.contains("${{");
    if ok {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_helper_path:{path}")))
    }
}

/// Preview roots are fixed runner-temp templates, never the repo.
pub(crate) fn validate_output_dir(dir: &str) -> Result<(), RenderError> {
    let ok = dir.starts_with("$RUNNER_TEMP/")
        && !dir.contains(' ')
        && !dir.contains('\n')
        && !dir.split('/').any(|seg| seg.is_empty() || seg == "..");
    if ok {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_output_dir:{dir}")))
    }
}
