//! Strict data transport before registry index assembly.

use super::{OciRenderContext, scripts, steps};
use velnor_actions_contract::{CompiledSourceHelper, Step, config::OciImage};
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn downloads(
    context: &OciRenderContext,
    image: &OciImage,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    image
        .platforms
        .iter()
        .map(|platform| download(context, image, platform, records))
        .collect()
}

fn download(
    context: &OciRenderContext,
    image: &OciImage,
    platform: &velnor_actions_contract::config::OciPlatform,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Step, RenderError> {
    let mut env = steps::identity(image);
    env.extend([
        ("ARCH", platform.arch().to_owned()),
        ("GH_TOKEN", "${{ github.token }}".to_owned()),
        ("REPOSITORY", "${{ github.repository }}".to_owned()),
        (
            "ARTIFACT_JOB",
            format!("platform-{}-{}", image.id, platform.arch()),
        ),
        (
            "ARTIFACT_ID",
            format!(
                "${{{{ needs.platform-{}-{}.outputs.artifact_id }}}}",
                image.id,
                platform.arch()
            ),
        ),
        (
            "ARTIFACT_DIGEST",
            format!(
                "sha256:${{{{ needs.platform-{}-{}.outputs.artifact_digest }}}}",
                image.id,
                platform.arch()
            ),
        ),
    ]);
    let mut step = steps::shell(
        context,
        "Verify raw immutable artifact archive",
        &format!("artifact_{}", platform.arch()),
        scripts::artifact_script(),
        env,
        "ubuntu-24.04",
        records,
    )?;
    step.condition = Some(format!(
        "${{{{ needs.admit-{}.outputs.existing != 'true' }}}}",
        image.id
    ));
    Ok(step)
}
