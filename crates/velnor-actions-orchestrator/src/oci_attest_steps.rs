//! Isolated final attestation graph; no registry login or mutation here.
use super::{OciRenderContext, scripts, steps};
use std::collections::BTreeMap;
use velnor_actions_contract::config::{OciImage, OciReleaseConfig};
use velnor_actions_contract::workflow::{
    NativeOciPublishEnvironment, NativePublishBinding, NativePublishPreparation, NativePublishRole,
};
use velnor_actions_contract::{
    CompiledSourceHelper, Job, PermissionLevel, Permissions, StepId, StepKind,
};
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn job(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    jobs: &BTreeMap<String, Job>,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Job, RenderError> {
    let mut sequence = super::bound_steps::setup(context, "ubuntu-24.04", records)?;
    let mut preparation = Vec::new();
    for (index, step) in sequence.iter_mut().enumerate() {
        let id = StepId::new(&format!("prepare_{index}")).map_err(RenderError::Contract)?;
        step.id = Some(id.clone());
        if let StepKind::SourceBoundHelper { invocation, env } = &step.kind {
            preparation.push(NativePublishPreparation {
                step_id: id,
                invocation: invocation.clone(),
                environment: env.clone(),
            });
        }
    }
    sequence.push(steps::shell(
        context,
        "Admit current complete CI source",
        "publish_admission",
        scripts::publish_admission_script(
            &context.repository,
            &context.ci_workflow,
            &context.default_branch,
        ),
        source_env(),
        "ubuntu-24.04",
        records,
    )?);
    sequence.push(steps::shell(
        context,
        "Verify immutable index receipt",
        "receipt",
        scripts::index_receipt_script(),
        receipt_env(image),
        "ubuntu-24.04",
        records,
    )?);
    let role = role(config, context, image, jobs, &sequence, preparation)?;
    let mut attest = steps::attest_index(config, context, image);
    attest.id = Some(StepId::new("attest").map_err(RenderError::Contract)?);
    sequence.push(attest);
    let mut result = super::jobs::job(
        &format!("attest-{}", image.id),
        vec!["verify".into(), format!("image-{}", image.id)],
        sequence,
    )?;
    result.timeout_minutes = velnor_actions_contract::JobTimeout::RELEASE;
    result.permissions = Some(Permissions {
        contents: PermissionLevel::Read,
        actions: PermissionLevel::Read,
        id_token: PermissionLevel::Write,
        attestations: PermissionLevel::Write,
        ..Permissions::default()
    });
    result.condition = Some(role.condition());
    result.native_publish = Some(role);
    Ok(result)
}

fn helper_invocation(
    step: &velnor_actions_contract::Step,
) -> Result<velnor_actions_contract::HelperInvocation, RenderError> {
    match &step.kind {
        StepKind::SourceBoundHelper { invocation, .. } => Ok(invocation.clone()),
        _ => Err(RenderError::InvalidWorkflow(
            "oci_role_helper_missing".into(),
        )),
    }
}

fn helper_environment(
    step: &velnor_actions_contract::Step,
) -> Result<BTreeMap<String, String>, RenderError> {
    match &step.kind {
        StepKind::SourceBoundHelper { env, .. } => Ok(env.clone()),
        _ => Err(RenderError::InvalidWorkflow(
            "oci_role_environment_missing".into(),
        )),
    }
}

fn role(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    jobs: &BTreeMap<String, Job>,
    sequence: &[velnor_actions_contract::Step],
    preparation: Vec<NativePublishPreparation>,
) -> Result<NativePublishRole, RenderError> {
    let full_ci = jobs
        .get("verify")
        .and_then(|j| {
            j.steps
                .iter()
                .find(|s| s.id.as_ref().is_some_and(|id| id.as_str() == "verify"))
        })
        .ok_or_else(|| RenderError::InvalidWorkflow("oci_full_ci_missing".into()))?;
    Ok(NativePublishRole::DockerHubIndex {
        binding: NativePublishBinding {
            repository: context.repository.clone(),
            default_branch: context.default_branch.clone(),
            full_ci_job: "verify".into(),
            full_ci_admission: helper_invocation(full_ci)?,
            artifact_job: format!("image-{}", image.id),
            artifact_id_output: "artifact_id".into(),
            artifact_digest_output: "artifact_digest".into(),
            preparation,
            admission_step: StepId::new("publish_admission").map_err(RenderError::Contract)?,
            admission: helper_invocation(&sequence[sequence.len() - 2])?,
            receipt_step: StepId::new("receipt").map_err(RenderError::Contract)?,
            receipt: helper_invocation(&sequence[sequence.len() - 1])?,
            attest_step: StepId::new("attest").map_err(RenderError::Contract)?,
            attest_uses: context.pins.attest.clone(),
        },
        subject_name: if image.image.starts_with(&format!("{}/", config.registry)) {
            image.image.clone()
        } else {
            format!("{}/{}", config.registry, image.image)
        },
        environment: NativeOciPublishEnvironment {
            image: image.image.clone(),
            image_id: image.id.clone(),
            platforms: image
                .platforms
                .iter()
                .map(|p| p.arch().to_owned())
                .collect(),
            full_ci: helper_environment(full_ci)?,
            admission: helper_environment(&sequence[sequence.len() - 2])?,
            receipt: helper_environment(&sequence[sequence.len() - 1])?,
        },
        subject_digest_output: "index_digest".into(),
    })
}

fn source_env() -> Vec<(&'static str, String)> {
    vec![
        ("GH_TOKEN", String::from("${{ github.token }}")),
        ("REF", String::from("${{ github.ref }}")),
        ("SOURCE_SHA", String::from("${{ github.sha }}")),
        ("REPOSITORY", String::from("${{ github.repository }}")),
    ]
}

fn receipt_env(image: &OciImage) -> Vec<(&'static str, String)> {
    let mut env = steps::identity(image);
    env.extend(source_env());
    env.extend([
        (
            "PLATFORMS",
            String::from(
                image
                    .platforms
                    .iter()
                    .map(|p| p.arch())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ),
        ("ARTIFACT_JOB", String::from(format!("image-{}", image.id))),
        (
            "ARTIFACT_ID",
            String::from(format!(
                "${{{{ needs.image-{}.outputs.artifact_id }}}}",
                image.id
            )),
        ),
        (
            "ARTIFACT_DIGEST",
            String::from(format!(
                "sha256:${{{{ needs.image-{}.outputs.artifact_digest }}}}",
                image.id
            )),
        ),
    ]);
    env
}
