//! Authoritative Pages composition seals catalog actions and full CI admission.
use super::{AptRenderContext, jobs, records::AptRecords, steps, tools};
use std::collections::BTreeMap;
use velnor_actions_actionlint::actions::{
    CHECKOUT_ACTION_SHA, CONFIGURE_PAGES_ACTION_SHA, DEPLOY_PAGES_ACTION_SHA,
    UPLOAD_PAGES_ARTIFACT_ACTION_SHA,
};
use velnor_actions_contract::workflow::{
    PermissionLevel, Permissions,
    pages::{
        NativePagesActions, NativePagesDeploy, NativePagesPreparation, NativePagesTriggerPolicy,
    },
};
use velnor_actions_contract::{
    CompiledSourceHelper, Job, StepId, StepKind, config::AptDeliveryConfig,
};
use velnor_actions_workflow_renderer::{RenderError, pages_approval::NativePagesApproval};

pub(super) fn deploy(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
    records: &AptRecords,
    registry: &mut Vec<CompiledSourceHelper>,
) -> Result<(Job, NativePagesApproval), RenderError> {
    let actions = NativePagesActions {
        checkout: format!("actions/checkout@{CHECKOUT_ACTION_SHA}"),
        configure: format!("actions/configure-pages@{CONFIGURE_PAGES_ACTION_SHA}"),
        upload: format!("actions/upload-pages-artifact@{UPLOAD_PAGES_ARTIFACT_ACTION_SHA}"),
        deploy: format!("actions/deploy-pages@{DEPLOY_PAGES_ACTION_SHA}"),
    };
    let checkout = steps::action(
        "Checkout",
        &actions.checkout,
        BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("ref".to_owned(), "${{ github.sha }}".to_owned()),
        ]),
    );
    let mut admission = jobs::helper("Verify immutable feed and guard rollback", &records.pages)?;
    admission.id = Some(id("pages_admission")?);
    let mut deployment = steps::action("Deploy Pages", &actions.deploy, BTreeMap::new());
    deployment.id = Some(id("pages_deploy")?);
    let mut job = steps::job(
        "Deploy apt feed",
        &context.workflow.runs_on,
        20,
        vec![
            checkout,
            admission,
            steps::action("Configure Pages", &actions.configure, BTreeMap::new()),
            steps::action(
                "Upload Pages artifact",
                &actions.upload,
                BTreeMap::from([("path".to_owned(), "public".to_owned())]),
            ),
            deployment,
        ],
    )?;
    job = tools::prepare(job, context, false, registry)?;
    let preparation = preparation(&mut job)?;
    let role = role(config, records, actions, preparation)?;
    job.condition = Some(role.condition());
    job.needs = ["stage", "admit"].map(str::to_owned).to_vec();
    job.environment = Some("github-pages".to_owned());
    job.permissions = Some(Permissions {
        contents: PermissionLevel::Read,
        actions: PermissionLevel::Read,
        pages: PermissionLevel::Write,
        id_token: PermissionLevel::Write,
        ..Permissions::default()
    });
    job.native_pages_deploy = Some(role.clone());
    Ok((job, NativePagesApproval::compiled(role)))
}

fn preparation(job: &mut Job) -> Result<Vec<NativePagesPreparation>, RenderError> {
    let mut preparation = Vec::new();
    for (index, step) in job.steps.iter_mut().enumerate().take(3).skip(1) {
        step.id = Some(id(if index == 1 {
            "pages_bootstrap"
        } else {
            "pages_tools"
        })?);
        let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(
                "apt_pages_preparation_not_source_bound".to_owned(),
            ));
        };
        preparation.push(NativePagesPreparation {
            step_id: step.id.clone().ok_or_else(|| {
                RenderError::InvalidWorkflow("apt_pages_preparation_id".to_owned())
            })?,
            invocation: invocation.clone(),
            environment: env.clone(),
        });
    }
    Ok(preparation)
}

fn role(
    config: &AptDeliveryConfig,
    records: &AptRecords,
    actions: NativePagesActions,
    preparation: Vec<NativePagesPreparation>,
) -> Result<NativePagesDeploy, RenderError> {
    let mut extra = records.pages.environment().clone();
    for key in [
        "APPROVED_REPOSITORY",
        "APPROVED_DEFAULT_BRANCH",
        "APPROVED_SOURCE_SHA",
        "FULL_CI_RESULT",
        "EXPECTED_ARTIFACT_ID",
        "EXPECTED_ARTIFACT_DIGEST",
        "GH_TOKEN",
    ] {
        extra.remove(key);
    }
    Ok(NativePagesDeploy {
        repository: config.consumer_repository.clone(),
        default_branch: config.branch.clone(),
        trigger_policy: NativePagesTriggerPolicy::ScheduledExplicitPublish,
        preparation,
        full_ci_job: "admit".to_owned(),
        full_ci_admission: records.admit.invocation().clone(),
        artifact_job: "stage".to_owned(),
        artifact_id_output: "artifact_id".to_owned(),
        artifact_digest_output: "artifact_digest".to_owned(),
        admission_step: id("pages_admission")?,
        admission: records.pages.invocation().clone(),
        admission_extra_environment: extra,
        deploy_step: id("pages_deploy")?,
        actions,
    })
}

fn id(value: &str) -> Result<StepId, RenderError> {
    StepId::new(value).map_err(RenderError::Contract)
}
