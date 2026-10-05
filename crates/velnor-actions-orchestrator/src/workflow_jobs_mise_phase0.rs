//! Required jobs that qualify official Mise `miserc` isolation behavior.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};
use velnor_actions_workflow_renderer::{ambient_shell_step, shell_step};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Phase0 jobs are required on each supported official Mise binary platform.
pub(super) const JOB_IDS: [&str; 3] = [
    velnor_actions_contract::MISE_PHASE0_LINUX_JOB_ID,
    velnor_actions_contract::MISE_PHASE0_MACOS_ARM64_JOB_ID,
    velnor_actions_contract::MISE_PHASE0_MACOS_INTEL_JOB_ID,
];

/// Build the platform-specific official-binary qualification jobs.
///
/// # Errors
///
/// Returns a typed-step error if the pinned setup or script command is invalid.
pub(super) fn jobs(catalog: &ToolCatalog) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let linux = qualification_job("Linux x64", "ubuntu-26.04", catalog, true)?;
    let macos_arm = qualification_job("macOS ARM64", "macos-15", catalog, true)?;
    let macos_intel = qualification_job("macOS Intel", "macos-15-intel", catalog, false)?;
    Ok(vec![
        (JOB_IDS[0].to_owned(), linux),
        (JOB_IDS[1].to_owned(), macos_arm),
        (JOB_IDS[2].to_owned(), macos_intel),
    ])
}

fn qualification_job(
    platform: &str,
    runner: &str,
    catalog: &ToolCatalog,
    include_mbx: bool,
) -> Result<Job, OrchestratorError> {
    let mut tools = vec![PinnedTool::Rust];
    if include_mbx {
        tools.push(PinnedTool::MrBoxington);
    }
    tools.push(PinnedTool::Nextest);
    let prepare = PreparePinnedTools::new(tools, ToolHomes::runner_temp()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let prepare_run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let prepare_env = strings_of_env(&prepare.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let prepare_step = ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, prepare_run, prepare_env)
        .map_err(OrchestratorError::from)?;
    let tests = shell_step(
        "Run official-binary Mise Phase0 qualification",
        vec!["bash".to_owned(), ".mise/tasks/mise-phase0".to_owned()],
        BTreeMap::new(),
    )
    .map_err(OrchestratorError::from)?;
    let checkout = crate::workflow::wire_w1::checkout_step()?;
    Ok(Job {
        display_name: format!("Mise Phase0 / {platform}"),
        runs_on: runner.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checkout, prepare_step, tests],
    })
}

#[cfg(test)]
#[path = "workflow_jobs_mise_phase0_tests.rs"]
mod tests;
