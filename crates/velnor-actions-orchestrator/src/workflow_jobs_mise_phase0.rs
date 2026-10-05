//! Required real-binary Phase0 qualification jobs for Mise.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{Job, JobTimeout, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::OrchestratorError;
use crate::utf8::strings_of;

/// Linux x64 real-binary qualification job.
pub(crate) const LINUX_X64_JOB_ID: &str = "mise-phase0-linux-x64";
/// macOS ARM64 real-binary qualification job.
pub(crate) const MACOS_ARM64_JOB_ID: &str = "mise-phase0-macos-arm64";

/// Construct native Linux x64 and macOS ARM64 real-binary jobs.
///
/// # Errors
///
/// Returns contract or renderer errors when a typed step is rejected.
pub(crate) fn phase0_jobs(catalog: &ToolCatalog) -> Result<Vec<(String, Job)>, OrchestratorError> {
    Ok(vec![
        (
            LINUX_X64_JOB_ID.to_owned(),
            phase0_job("Mise Phase0 / Linux x64", "ubuntu-26.04", catalog)?,
        ),
        (
            MACOS_ARM64_JOB_ID.to_owned(),
            phase0_job("Mise Phase0 / macOS arm64", "macos-15", catalog)?,
        ),
    ])
}

/// Insert Phase0 jobs only into the Velnor repository's own workflow.
pub(crate) fn insert_jobs(
    ir_jobs: &mut BTreeMap<String, Job>,
    policy: WorkflowPolicy,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    if policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for (id, job) in phase0_jobs(catalog)? {
        ids.push(id.clone());
        ir_jobs.insert(id, job);
    }
    Ok(ids)
}

/// One native runner job installing pinned Cargo/Nextest and running the suite.
fn phase0_job(
    display_name: &str,
    runs_on: &str,
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    let prepare =
        crate::matrix_step::prepare_crate_tools_step(catalog, true, false, true, false, false)?;
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::Nextest],
        OsStr::new("bash"),
        vec![OsString::from(".mise/tasks/mise-phase0")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let run = strings_of(exec.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    let test = velnor_actions_workflow_renderer::shell_step(
        "Download pinned Mise binaries and run Phase0 tests",
        run,
        env,
    )?;
    Ok(Job {
        display_name: display_name.to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![crate::workflow::wire_w1::checkout_step()?, prepare, test],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    #[test]
    fn phase0_jobs_are_native_and_run_the_required_script_under_pinned_tools() {
        let jobs = phase0_jobs(&ToolCatalog::pinned()).expect("phase0 jobs");
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].0, LINUX_X64_JOB_ID);
        assert_eq!(jobs[0].1.runs_on, "ubuntu-26.04");
        assert_eq!(jobs[1].0, MACOS_ARM64_JOB_ID);
        assert_eq!(jobs[1].1.runs_on, "macos-15");
        for (_, job) in jobs {
            let test = job.steps.last().expect("test step is last");
            let StepKind::Shell { run, .. } = &test.kind else {
                panic!("Phase0 test uses a shell invocation");
            };
            assert!(run.iter().any(|arg| arg == ".mise/tasks/mise-phase0"));
            assert!(run.iter().any(|arg| arg.ends_with("cargo-nextest@0.9.146")));
            assert!(run.iter().any(|arg| arg == "rust@1.98.1"));
        }
    }

    #[test]
    fn consumer_policy_does_not_add_repository_phase0_jobs() {
        let mut ir_jobs = BTreeMap::new();
        let ids = insert_jobs(
            &mut ir_jobs,
            WorkflowPolicy::ConsumerV1,
            &ToolCatalog::pinned(),
        )
        .expect("consumer job selection");
        assert!(ids.is_empty());
        assert!(ir_jobs.is_empty());
    }
}
