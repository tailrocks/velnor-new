//! Resolve native-image tasks to a tracked, bounded script source.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path};

use velnor_actions_contract::{NativeImageTask, VelnorConfig, WorkflowTask};
use velnor_actions_mise::GitRequest;
use velnor_actions_workflow_renderer::verification_jobs::NativeImageTaskPolicy;

use crate::OrchestratorError;
use crate::safe_read::{RepoRead, read_repo_file};

const MAX_NATIVE_IMAGE_SCRIPT_BYTES: u64 = 1024 * 1024;

/// Resolve the one declared image task against its checked-out script bytes.
pub(crate) fn policies(
    root: &Path,
    config: &VelnorConfig,
) -> Result<Vec<NativeImageTaskPolicy>, OrchestratorError> {
    config
        .workflow
        .tasks
        .iter()
        .filter_map(|task| match task {
            WorkflowTask::NativeImage(image) => Some(image),
            WorkflowTask::Verification(_) | WorkflowTask::Build(_) => None,
        })
        .map(|task| resolve_policy(root, task))
        .collect()
}

fn resolve_policy(
    root: &Path,
    task: &NativeImageTask,
) -> Result<NativeImageTaskPolicy, OrchestratorError> {
    task.validate(".velnor/config.toml")
        .map_err(|_| failure("native_image_task_contract"))?;
    validate_script_components(root, &task.script)?;
    require_tracked_script(root, &task.script)?;
    let script = match read_repo_file(root, &task.script, MAX_NATIVE_IMAGE_SCRIPT_BYTES)? {
        RepoRead::Text(script) => script,
        RepoRead::Absent => return Err(failure("native_image_script_missing")),
    };
    let source_sha256 = crate::cover_identity::generator::sha256_hex(script.as_bytes());
    let runner_label = task.platform.runs_on().to_owned();
    Ok(NativeImageTaskPolicy {
        task: task.clone(),
        runner_label,
        source_sha256,
    })
}

fn validate_script_components(root: &Path, script: &str) -> Result<(), OrchestratorError> {
    let components = Path::new(script).components().collect::<Vec<_>>();
    if components.len() < 2 {
        return Err(failure("native_image_script_path"));
    }
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(failure("native_image_script_path"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| failure("native_image_script_missing_or_unreadable"))?;
        if metadata.file_type().is_symlink() {
            return Err(failure("native_image_script_symlink"));
        }
        let last = index + 1 == components.len();
        if (last && !metadata.is_file()) || (!last && !metadata.is_dir()) {
            return Err(failure("native_image_script_not_regular_file"));
        }
    }
    Ok(())
}

fn require_tracked_script(root: &Path, script: &str) -> Result<(), OrchestratorError> {
    let output = GitRequest::ls_files(vec![
        OsString::from("--error-unmatch"),
        OsString::from("--"),
        OsString::from(script),
    ])
    .run_in(root)
    .map_err(|_| failure("native_image_script_tracking_check"))?;
    if !output.success {
        return Err(failure("native_image_script_must_be_tracked"));
    }
    let listed = output
        .stdout_text("git")
        .map_err(|_| failure("native_image_script_tracking_check"))?;
    if listed.trim_end_matches('\n') != script {
        return Err(failure("native_image_script_must_be_tracked"));
    }
    Ok(())
}

fn failure(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_NATIVE_IMAGE_SCRIPT_BYTES, validate_script_components};

    #[test]
    fn native_image_script_is_regular_and_uses_only_non_symlink_directories() {
        let root = tempfile::tempdir().expect("temporary repo");
        let directory = root.path().join("maintained-image-build");
        std::fs::create_dir(&directory).expect("script directory");
        std::fs::write(directory.join("check.sh"), "#!/bin/bash\n").expect("script");
        assert!(validate_script_components(root.path(), "maintained-image-build/check.sh").is_ok());

        let symlink_root = tempfile::tempdir().expect("symlink repo");
        let outside = tempfile::tempdir().expect("outside directory");
        std::fs::write(outside.path().join("check.sh"), "#!/bin/bash\n").expect("outside script");
        std::os::unix::fs::symlink(
            outside.path(),
            symlink_root.path().join("maintained-image-build"),
        )
        .expect("directory symlink");
        assert!(
            validate_script_components(symlink_root.path(), "maintained-image-build/check.sh")
                .is_err()
        );

        assert_eq!(MAX_NATIVE_IMAGE_SCRIPT_BYTES, 1024 * 1024);
    }
}
