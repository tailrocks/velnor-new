//! OCI jobs bind exact native hosts before compiled SDK admission.
use super::{OciRenderContext, scripts, steps};
use std::collections::BTreeMap;
use velnor_actions_contract::config::{OciImage, OciReleaseConfig};
use velnor_actions_contract::workflow::{JobOutput, StepOutputRef};
use velnor_actions_contract::{
    ActionOutput, CompiledSourceHelper, Job, JobTimeout, PermissionLevel, Permissions, Step, StepId,
};
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn job(id: &str, needs: Vec<String>, sequence: Vec<Step>) -> Result<Job, RenderError> {
    Ok(Job {
        cache_mode: None,
        display_name: id.into(),
        runs_on: "ubuntu-24.04".into(),
        timeout_minutes: JobTimeout::new(60).map_err(RenderError::Contract)?,
        needs,
        condition: None,
        permissions: Some(read_permissions()),
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: sequence,
    })
}
pub(super) fn read_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        actions: PermissionLevel::Read,
        ..Permissions::default()
    }
}
pub(super) fn output(
    name: &str,
    step: &str,
    value: ActionOutput,
) -> Result<JobOutput, RenderError> {
    Ok(JobOutput {
        name: name.into(),
        value: StepOutputRef {
            step_id: StepId::new(step).map_err(RenderError::Contract)?,
            output: value,
        },
    })
}
fn verify(
    context: &OciRenderContext,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Job, RenderError> {
    let mut sequence = vec![steps::checkout(context, false)];
    sequence.extend(super::bound_steps::setup(context, "ubuntu-24.04", records)?);
    sequence.push(steps::shell(
        context,
        "Verify exact release source and complete CI",
        "verify",
        scripts::verify_script(
            &context.repository,
            &context.ci_workflow,
            &context.default_branch,
        ),
        vec![
            ("GH_TOKEN", "${{ github.token }}".into()),
            ("EVENT_NAME", "${{ github.event_name }}".into()),
            ("REF", "${{ github.ref }}".into()),
            ("SOURCE_SHA", "${{ github.sha }}".into()),
            ("REPOSITORY", "${{ github.repository }}".into()),
        ],
        "ubuntu-24.04",
        records,
    )?);
    let mut result = job("verify", Vec::new(), sequence)?;
    result.condition = Some(format!(
        "success() && github.repository == '{}' && startsWith(github.ref, 'refs/tags/v') && (github.event_name == 'push' || github.event_name == 'workflow_dispatch')",
        context.repository
    ));
    result.outputs = vec![output("version", "verify", ActionOutput::ReleaseVersion)?];
    Ok(result)
}
fn admission(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Job, RenderError> {
    let mut needs = vec!["verify".into()];
    needs.extend(image.depends_on.iter().map(|id| format!("image-{id}")));
    let mut result = job(
        &format!("admit-{}", image.id),
        needs,
        steps::admission(config, context, image, records)?,
    )?;
    result.outputs = vec![
        output("existing", "admit", ActionOutput::OciExisting)?,
        output("index_digest", "admit", ActionOutput::OciIndexDigest)?,
    ];
    Ok(result)
}
fn build(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    arch: &str,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Job, RenderError> {
    let mut result = job(
        &format!("platform-{}-{arch}", image.id),
        vec!["verify".into(), format!("admit-{}", image.id)],
        steps::build(config, context, image, arch, runner, records)?,
    )?;
    result.outputs = vec![
        output("artifact_id", "digestproof", ActionOutput::ArtifactId)?,
        output(
            "artifact_digest",
            "digestproof",
            ActionOutput::ArtifactDigest,
        )?,
    ];
    result.runs_on = runner.into();
    result.condition = Some(format!(
        "needs.verify.result == 'success' && needs.admit-{}.outputs.existing == 'false'",
        image.id
    ));
    Ok(result)
}
fn assemble(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Job, RenderError> {
    let mut needs = vec!["verify".into(), format!("admit-{}", image.id)];
    needs.extend(
        image
            .platforms
            .iter()
            .map(|p| format!("platform-{}-{}", image.id, p.arch())),
    );
    let successful = image
        .platforms
        .iter()
        .map(|p| {
            format!(
                "needs.platform-{}-{}.result == 'success'",
                image.id,
                p.arch()
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    let skipped = image
        .platforms
        .iter()
        .map(|p| {
            format!(
                "needs.platform-{}-{}.result == 'skipped'",
                image.id,
                p.arch()
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    let mut result = job(
        &format!("image-{}", image.id),
        needs,
        steps::assemble(config, context, image, records)?,
    )?;
    result.condition = Some(format!(
        "always() && needs.verify.result == 'success' && needs.admit-{}.result == 'success' && (({successful}) || (({skipped}) && needs.admit-{}.outputs.existing == 'true'))",
        image.id, image.id
    ));
    result.outputs = vec![
        output("index_digest", "assemble", ActionOutput::OciIndexDigest)?,
        output("artifact_id", "indexproof", ActionOutput::ArtifactId)?,
        output(
            "artifact_digest",
            "indexproof",
            ActionOutput::ArtifactDigest,
        )?,
    ];
    Ok(result)
}
pub(super) fn render(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<BTreeMap<String, Job>, RenderError> {
    let mut jobs = BTreeMap::from([("verify".into(), verify(context, records)?)]);
    for image in &config.images {
        jobs.insert(
            format!("admit-{}", image.id),
            admission(config, context, image, records)?,
        );
        for platform in &image.platforms {
            jobs.insert(
                format!("platform-{}-{}", image.id, platform.arch()),
                build(
                    config,
                    context,
                    image,
                    platform.arch(),
                    platform.runner(),
                    records,
                )?,
            );
        }
        jobs.insert(
            format!("image-{}", image.id),
            assemble(config, context, image, records)?,
        );
        let attest = super::attest_steps::job(config, context, image, &jobs, records)?;
        jobs.insert(format!("attest-{}", image.id), attest);
    }
    Ok(jobs)
}
