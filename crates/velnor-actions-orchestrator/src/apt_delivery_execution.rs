//! Exact SDK execution admission; unit fixtures provide no distribution proof.
use super::AptRenderContext;
use velnor_actions_contract::workflow::native_tools::{
    CompiledNativeExecRecipe, NativeCredentialScope,
};
use velnor_actions_workflow_renderer::RenderError;

#[cfg(not(test))]
pub(super) fn recipe(
    context: &AptRenderContext,
    scope: NativeCredentialScope,
) -> Result<CompiledNativeExecRecipe, RenderError> {
    use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};
    let host =
        match velnor_actions_contract::tool_target_for_runner_label(&context.workflow.runs_on) {
            Some("x86_64-unknown-linux-gnu") => DistributionHost::LinuxAmd64,
            Some("aarch64-unknown-linux-gnu") => DistributionHost::LinuxArm64,
            _ => {
                return Err(RenderError::InvalidWorkflow(
                    "apt_sdk_execution_host".to_owned(),
                ));
            }
        };
    delivery_tools::execution_recipe(host, scope)
        .map_err(|error| RenderError::BadCommand(format!("apt_execution_authority:{error}")))
}

// Structural graph equality fixture only. No owned distribution or hosted proof.
#[cfg(test)]
pub(super) fn recipe(
    context: &AptRenderContext,
    scope: NativeCredentialScope,
) -> Result<CompiledNativeExecRecipe, RenderError> {
    let selectors = vec![
        format!("python@{}", context.tools.python_version),
        format!("gh@{}", context.tools.gh_version),
    ];
    let mut environment = context.tools.isolation_env_pairs();
    match scope {
        NativeCredentialScope::GithubReadOnly => {
            environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
        }
        NativeCredentialScope::AptSigning => {
            environment.insert(
                "APT_GPG_PRIVATE_KEY".to_owned(),
                "${{ secrets.APT_GPG_PRIVATE_KEY }}".to_owned(),
            );
            environment.insert(
                "APT_GPG_PASSPHRASE".to_owned(),
                "${{ secrets.APT_GPG_PASSPHRASE }}".to_owned(),
            );
        }
        NativeCredentialScope::Anonymous => {}
        NativeCredentialScope::AppleSigning
        | NativeCredentialScope::GithubIssueWrite
        | NativeCredentialScope::RustRegistryPublishOidc
        | NativeCredentialScope::RustRegistryPublishBootstrap
        | NativeCredentialScope::GithubReleasePublish
        | NativeCredentialScope::OciRegistryPublish => {
            return Err(RenderError::InvalidWorkflow(
                "apt_fixture_credential_role".to_owned(),
            ));
        }
    }
    let mut prefix = [
        "/usr/bin/env",
        "-i",
        "/fixture-only/mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
    ]
    .map(str::to_owned)
    .to_vec();
    prefix.extend(selectors.iter().cloned());
    prefix.push("--".to_owned());
    CompiledNativeExecRecipe::compiled_for_scope(prefix, environment, selectors, scope)
        .map_err(RenderError::Contract)
}
