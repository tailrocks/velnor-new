use crate::{OrchestratorError, prepare::GenerationPreparation};
use velnor_actions_contract::{
    ReleaseTarget,
    config::{CheckExecutor, CheckPlatform, CheckRunner},
    review_completion::{REVIEW_COMPLETION_POLICY_SCHEMA, ReviewCompletionPolicy},
};
use velnor_actions_mise::{MiseInstall, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::{
    render::RenderedFile,
    review_completion::{ReviewCompletionSpec, render_review_completion},
};

pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(config) = prep.config.workflow.review_completion.as_ref() else {
        return Ok(Vec::new());
    };
    let runner = CheckRunner {
        label: "ubuntu-26.04".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    };
    let install = MiseInstall::new(vec![PinnedTool::Gh]).map_err(|e| failure(e.to_string()))?;
    let spec = ReviewCompletionSpec {
        policy: ReviewCompletionPolicy {
            schema: REVIEW_COMPLETION_POLICY_SCHEMA.to_owned(),
            repository: crate::release_identity::origin_repository(&prep.root)?,
            default_branch: prep.default_branch.clone(),
            provider: config.provider,
            check_name: config.check_name.clone(),
        },
        config: config.clone(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        setup: crate::pins::resolve_mise_setup_for_release_target(
            &prep.config,
            ReleaseTarget::LinuxX86_64,
        )?,
        acquire: crate::pins::consumer_acquire_for_runner(
            &runner,
            env!("CARGO_PKG_VERSION"),
            prep.discovery.consumer_manifest_json.as_deref(),
        )?,
        install_gh_argv: crate::utf8::strings_of(install.argv(&ToolCatalog::pinned()))
            .map_err(failure)?,
    };
    Ok(vec![render_review_completion(&spec)?])
}
fn failure(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}
