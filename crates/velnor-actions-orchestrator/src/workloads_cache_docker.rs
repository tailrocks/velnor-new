//! BuildKit owns container layers; host archives and RUN mounts stay separate.

use velnor_actions_contract::{CrateJob, Step};

use crate::OrchestratorError;
use crate::internal_plan::workload_identity::action::{ActionInvocation, DockerTransport};

/// Reviewed Buildx action prepares the fixed isolated BuildKit owner.
pub(crate) fn prepare_step(model: &CrateJob) -> Result<Option<Step>, OrchestratorError> {
    if !is_docker(model) {
        return Ok(None);
    }
    let transport = DockerTransport::new(&model.manifest, &model.configuration)?;
    Ok(Some(action_step(
        "Prepare container builder",
        transport.setup,
    )?))
}

/// Validation imports layers on every event. Trusted export runs only after
/// the caller qualifies successful terminal and matrix reports for this source.
pub(crate) fn build_step(model: &CrateJob, write: bool) -> Result<Step, OrchestratorError> {
    if !is_docker(model) {
        return Err(crate::internal::internal("docker_cache_non_docker_model"));
    }
    let transport = DockerTransport::new(&model.manifest, &model.configuration)?;
    if write {
        action_step("Export container cache", transport.write)
    } else {
        action_step("Build", transport.read)
    }
}

fn is_docker(model: &CrateJob) -> bool {
    model.job_id.starts_with("workload-") && model.configuration == "docker_build"
}

fn action_step(name: &str, invocation: ActionInvocation) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::action_step_with_env(
        name,
        &invocation.uses,
        invocation.inputs,
        invocation.env,
    )?;
    step.condition = invocation.condition;
    Ok(step)
}
