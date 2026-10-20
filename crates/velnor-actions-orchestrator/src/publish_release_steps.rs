//! Rust `git-tag` release jobs of the publication workflow.
//!
//! A default-branch authority job waits for the `Required` verdict, then the
//! release job runs the pinned release-plz to create tags and GitHub
//! releases. It holds `contents: write` only: no registry token, no
//! `id-token`, no pull-request authority.

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

/// Default-branch authority job id; the tag release job needs it.
const BRANCH_AUTHORITY_JOB: &str = "publish-branch-authority";
/// Tag release job id.
const TAG_RELEASE_JOB: &str = "rust-tag-release";
/// Polls of the `Required` verdict and the pause between them.
const REQUIRED_POLLS: (u16, u16) = (60, 30);

/// Both release jobs, keyed by id.
///
/// With bundles the workflow also runs on tags, so the authority job is
/// conditional on the pushed ref being a branch.
pub(crate) fn release_jobs(
    inputs: &Inputs<'_>,
    gh: &str,
    manifest: &str,
    mixed: bool,
) -> Result<BTreeMap<String, PublishJobSpec>, OrchestratorError> {
    let mut authority = branch_authority_job(inputs, gh)?;
    if mixed {
        authority.condition = Some(event_kind("branch"));
    }
    Ok(BTreeMap::from([
        (BRANCH_AUTHORITY_JOB.to_owned(), authority),
        (
            TAG_RELEASE_JOB.to_owned(),
            tag_release_job(inputs, manifest)?,
        ),
    ]))
}

/// Condition selecting the authority job that matches the pushed ref type.
pub(crate) fn event_kind(kind: &str) -> String {
    format!("github.ref_type == '{kind}'")
}

/// Branch authority: a default-branch push whose `Required` check is green.
fn branch_authority_job(
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
    // Budget: every poll plus its pause, and a margin for setup.
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
    })
}

/// Tag release: pinned release-plz creates tags and GitHub releases only.
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
