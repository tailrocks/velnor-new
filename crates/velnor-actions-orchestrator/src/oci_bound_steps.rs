//! Bind immutable OCI source to exact SDK execution and a closed phase.
use super::{OciRenderContext, support::render_oci_support_files};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation, Step, StepId,
    workflow::native_tools::NativeCredentialScope,
};
use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn render(
    context: &OciRenderContext,
    name: &str,
    id: &str,
    script: &[String],
    env: Vec<(&str, String)>,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Step, RenderError> {
    let phase = script
        .first()
        .map(String::as_str)
        .ok_or_else(|| invalid("oci_phase"))?;
    let scope = match phase {
        "admission" | "assembly" | "platform-publish" => NativeCredentialScope::OciRegistryPublish,
        "record" => NativeCredentialScope::Anonymous,
        "verify" | "source" | "artifact" | "publish-admission" | "index-receipt" => {
            NativeCredentialScope::GithubReadOnly
        }
        _ => return Err(invalid("oci_phase")),
    };
    let host = match runner {
        "ubuntu-24.04" => DistributionHost::LinuxAmd64,
        "ubuntu-24.04-arm" => DistributionHost::LinuxArm64,
        _ => return Err(invalid("oci_actual_runner")),
    };
    let mut environment = env
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect::<BTreeMap<_, _>>();
    if scope == NativeCredentialScope::OciRegistryPublish {
        environment.insert(
            "DOCKER_CONFIG".into(),
            "${{ runner.temp }}/velnor/oci-docker".into(),
        );
    }
    for (key, value) in runtime_data() {
        environment.insert(key.to_owned(), value.to_owned());
    }
    let file = render_oci_support_files(&context.generator_version)?
        .into_iter()
        .find(|file| file.path == SourceBoundOperation::OciDelivery.path())
        .ok_or_else(|| invalid("oci_compiled_wrapper_missing"))?;
    let digest = Sha256::digest(file.bytes.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let helper =
        SourceBoundHelper::compiled(SourceBoundOperation::OciDelivery, &file.path, &digest)
            .map_err(RenderError::Contract)?;
    let invocation = HelperInvocation::compiled(helper, script.to_vec(), Vec::new())
        .map_err(RenderError::Contract)?;
    let recipe = delivery_tools::execution_recipe(host, scope)
        .map_err(|error| invalid(&error.to_string()))?;
    delivery_tools::validate_recipe(host, scope, &recipe)
        .map_err(|error| invalid(&error.to_string()))?;
    let mut record = CompiledSourceHelper::compiled(invocation, file.bytes)
        .map_err(RenderError::Contract)?
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .map_err(RenderError::Contract)?;
    if matches!(phase, "verify" | "admission" | "assembly" | "index-receipt") {
        record = record.with_github_output().map_err(RenderError::Contract)?;
    }
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        name,
        &record,
        record.environment().clone(),
    )?;
    step.id = Some(StepId::new(id).map_err(RenderError::Contract)?);
    records.push(record);
    Ok(step)
}

fn runtime_data() -> [(&'static str, &'static str); 5] {
    [
        ("GITHUB_RUN_ID", "${{ github.run_id }}"),
        ("GITHUB_RUN_ATTEMPT", "${{ github.run_attempt }}"),
        ("GITHUB_REPOSITORY", "${{ github.repository }}"),
        ("GITHUB_REF", "${{ github.ref }}"),
        ("GITHUB_EVENT_NAME", "${{ github.event_name }}"),
    ]
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(problem.to_owned())
}

pub(super) fn setup(
    context: &OciRenderContext,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    let host = match runner {
        "ubuntu-24.04" => DistributionHost::LinuxAmd64,
        "ubuntu-24.04-arm" => DistributionHost::LinuxArm64,
        _ => return Err(invalid("oci_actual_runner")),
    };
    let bootstrap = velnor_actions_mise::catalog::mise_acquisition::helper_for_domain(
        velnor_actions_contract::ToolCacheDomain::Full,
        host,
        &context.generator_version,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    let preparation = delivery_tools::preparation(host, &context.generator_version)
        .map_err(|error| invalid(&error.to_string()))?;
    [
        ("Acquire qualified Mise", bootstrap),
        ("Prepare exact OCI tools", preparation),
    ]
    .into_iter()
    .map(|(name, record)| {
        let step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
            name,
            &record,
            record.environment().clone(),
        )?;
        records.push(record);
        Ok(step)
    })
    .collect()
}
