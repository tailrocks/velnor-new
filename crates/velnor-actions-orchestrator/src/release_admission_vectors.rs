//! Exact source-owned full-CI admission for fresh release authority jobs.
use super::JobInputs;
use crate::OrchestratorError;
use velnor_actions_contract::Step;
/// Fresh forge admission uses only the compiled neutral owner executable.
pub(in crate::release_emit) fn forge_admission_record(
    inputs: &JobInputs<'_>,
) -> Result<velnor_actions_contract::CompiledSourceHelper, OrchestratorError> {
    use crate::release_emit::release_admission::{AdmissionIdentity, AdmissionMode, executable};
    use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
    use velnor_actions_mise::catalog::qualification::DistributionHost;
    let identity = AdmissionIdentity::approved(inputs.repository, inputs.branch, inputs.sha)?;
    let host = match velnor_actions_contract::tool_target_for_runner_label(inputs.label) {
        Some("x86_64-unknown-linux-gnu") => DistributionHost::LinuxAmd64,
        Some("aarch64-unknown-linux-gnu") => DistributionHost::LinuxArm64,
        _ => {
            return Err(OrchestratorError::Contract {
                problem: "release_admission_host".to_owned(),
            });
        }
    };
    executable(
        AdmissionMode::DefaultBranch,
        &identity,
        env!("CARGO_PKG_VERSION"),
        host,
        NativeCredentialScope::GithubReadOnly,
    )
    .map_err(Into::into)
}

pub(super) fn forge_admission(inputs: &JobInputs<'_>) -> Result<Step, OrchestratorError> {
    let record = forge_admission_record(inputs)?;
    velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Admit exact source full CI",
        &record,
        record.environment().clone(),
    )
    .map_err(Into::into)
}
