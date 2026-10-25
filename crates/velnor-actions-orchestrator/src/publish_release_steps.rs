use std::collections::BTreeMap;

use velnor_actions_contract::JobTimeout;
use velnor_actions_workflow_renderer::publish_scripts as script;
use velnor_actions_workflow_renderer::publish_spec::{
    PublishJobSpec, PublishPermissions, PublishRole,
};
use velnor_actions_workflow_renderer::release_tree::RELEASE_CONFIG_PATH;
use velnor_actions_workflow_renderer::{ambient_shell_step, mise_setup_step};

use crate::OrchestratorError;
use crate::publish_steps::{Inputs, bundle_job, plain, script_step};
use crate::release_checkouts::{forge_env, policy_checkout};
use crate::release_steps::{Phase, phase_argv};

pub(crate) const BRANCH_AUTHORITY_JOB: &str = "publish-branch-authority";
const TAG_RELEASE_JOB: &str = "rust-tag-release";
const REQUIRED_POLLS: (u16, u16) = (60, 30);

pub(crate) fn release_jobs(
    inputs: &Inputs<'_>,
    manifest: &str,
) -> Result<BTreeMap<String, PublishJobSpec>, OrchestratorError> {
    Ok(BTreeMap::from([(
        TAG_RELEASE_JOB.to_owned(),
        tag_release_job(inputs, manifest)?,
    )]))
}

pub(crate) fn event_kind(kind: &str) -> String {
    format!("github.ref_type == '{kind}'")
}

pub(crate) fn branch_authority_job(
    inputs: &Inputs<'_>,
    gh: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let (polls, pause) = REQUIRED_POLLS;
    let steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        plain(script_step(
            "Verify default branch push",
            script::branch_push_script(inputs.repository, inputs.branch),
            false,
        )?),
        plain(script_step(
            "Wait for Required check",
            script::required_wait_script(gh, polls, pause),
            true,
        )?),
    ];
    let minutes = u32::from(polls) * u32::from(pause) / 60 + 5;
    Ok(PublishJobSpec {
        role: PublishRole::BranchAuthority,
        display_name: "Verify default branch authority".to_owned(),
        runs_on: inputs.label.to_owned(),
        timeout_minutes: JobTimeout::try_from(minutes)?,
        needs: Vec::new(),
        condition: None,
        environment: None,
        outputs: BTreeMap::new(),
        permissions: PublishPermissions::expected(PublishRole::BranchAuthority),
        steps,
        services: Vec::new(),
    })
}

fn tag_release_job(
    inputs: &Inputs<'_>,
    manifest: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let argv = phase_argv(
        &Phase::Publish,
        RELEASE_CONFIG_PATH,
        manifest,
        None,
        inputs.catalog,
    )?;
    let steps = vec![
        plain(policy_checkout(true)?),
        plain(mise_setup_step(inputs.mise)?),
        plain(ambient_shell_step(
            "Create tags and releases",
            argv,
            forge_env(),
        )?),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::TagRelease,
        "Create tags and releases".to_owned(),
        BRANCH_AUTHORITY_JOB.to_owned(),
        steps,
    ))
}
