//! Exact native host binding for platform helper invocations.

use super::{OciRenderContext, bound_steps, scripts, steps};
use velnor_actions_contract::{CompiledSourceHelper, Step, config::OciImage};
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn source_gates(
    context: &OciRenderContext,
    arch: &str,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    let env = vec![
        ("GH_TOKEN", "${{ github.token }}".to_owned()),
        ("REF", "${{ github.ref }}".to_owned()),
        ("SOURCE_SHA", "${{ github.sha }}".to_owned()),
        ("REPOSITORY", "${{ github.repository }}".to_owned()),
    ];
    per_host(
        context,
        ("Recheck current protected source", "source"),
        &scripts::source_script(&context.repository, &context.default_branch),
        env,
        arch,
        runner,
        records,
    )
}

pub(super) fn records(
    context: &OciRenderContext,
    image: &OciImage,
    arch: &str,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    let mut env = steps::identity(image);
    env.push(("DIGEST", "${{ steps.build.outputs.digest }}".to_owned()));
    per_host(
        context,
        ("Record exact digest identity", "record"),
        &scripts::record_script(),
        env,
        arch,
        runner,
        records,
    )
}

fn per_host(
    context: &OciRenderContext,
    name_and_id: (&str, &str),
    script: &[String],
    mut env: Vec<(&str, String)>,
    arch: &str,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    env.push(("ARCH", arch.to_owned()));
    let step = bound_steps::render(
        context,
        name_and_id.0,
        &format!("{}_{arch}", name_and_id.1),
        script,
        env,
        runner,
        records,
    )?;
    Ok(vec![step])
}

pub(super) fn archive_path(image: &OciImage, arch: &str) -> String {
    format!("${{{{ runner.temp }}}}/velnor/oci-{}-{arch}.tar", image.id)
}

pub(super) fn publish(
    context: &OciRenderContext,
    image: &OciImage,
    arch: &str,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Step, RenderError> {
    let mut env = steps::identity(image);
    env.extend([
        ("ARCH", arch.into()),
        ("OCI_ARCHIVE", archive_path(image, arch)),
        ("DIGEST", "${{ steps.build.outputs.digest }}".into()),
        ("GH_TOKEN", "${{ github.token }}".into()),
        ("REF", "${{ github.ref }}".into()),
        ("REPOSITORY", "${{ github.repository }}".into()),
    ]);
    bound_steps::render(
        context,
        "Publish exact archive after current source recheck",
        "platform_publish",
        &scripts::platform_publish_script(
            &context.repository,
            &context.ci_workflow,
            &context.default_branch,
        ),
        env,
        runner,
        records,
    )
}
