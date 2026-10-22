//! Closed environment bindings for native release operations, never command input.
use super::super::TapInvocationContext;
use super::{ConsumerReleaseSpec, RenderError, Yaml};
use velnor_actions_contract::package_release::PackageReleaseChannel;
fn push(env: &mut Vec<(String, Yaml)>, key: &str, value: impl Into<String>) {
    env.push((key.to_owned(), Yaml::str(value.into())));
}
pub(super) fn environment(
    spec: &ConsumerReleaseSpec,
    op: &str,
    authenticated: bool,
    context: TapInvocationContext,
) -> Result<Yaml, RenderError> {
    let plan = String::from_utf8(
        spec.policy
            .canonical_bytes()
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut env = Vec::new();
    push(&mut env, "VELNOR_RELEASE_POLICY", plan);
    push(&mut env, "VELNOR_INTERNAL_OP", op);
    push(
        &mut env,
        "VELNOR_REQUEST_FILE",
        if op == "execute-release-producer-v1" {
            "${{ runner.temp }}/velnor/package-release-plan.json"
        } else {
            "${{ runner.temp }}/velnor/package-release-policy.json"
        },
    );
    for (key, value) in context.release_inputs() {
        push(&mut env, key, value);
    }
    channel_bindings(&mut env, spec, op, context)?;
    if matches!(
        op,
        "recover-package-release-v1"
            | "admit-homebrew-tap-v1"
            | "prepare-homebrew-tap-v1"
            | "write-homebrew-tap-pr-v1"
            | "merge-homebrew-tap-pr-v1"
    ) {
        for (key, value) in context.original() {
            push(&mut env, key, value);
        }
    }
    if spec.policy.channel == PackageReleaseChannel::Preview
        && matches!(
            op,
            "sign-package-release-v1"
                | "publish-package-release-v1"
                | "admit-release-aggregate-v1"
                | "acquire-release-leaves-v1"
        )
    {
        push(
            &mut env,
            "VELNOR_RELEASE_POLICY_ADMISSION_DIGEST",
            "${{ steps.policy-admission.outputs.admission_digest }}",
        );
    }
    if authenticated {
        push(&mut env, "GH_TOKEN", "${{ github.token }}");
    }
    if matches!(
        op,
        "publish-package-release-v1"
            | "recover-package-release-v1"
            | "write-stable-tag-intent-v1"
            | "write-stable-tag-origin-v1"
            | "select-stable-release-source-v1"
            | "admit-stable-tag-origin-v1"
            | "admit-homebrew-tap-v1"
            | "write-homebrew-tap-pr-v1"
            | "merge-homebrew-tap-pr-v1"
            | "select-release-tap-continuation-v1"
    ) {
        push(
            &mut env,
            "VELNOR_RELEASE_STATUS_TOKEN",
            "${{ secrets.RELEASE_STATUS_READ_TOKEN }}",
        );
    }
    Ok(Yaml::Map(env))
}
fn channel_bindings(
    env: &mut Vec<(String, Yaml)>,
    spec: &ConsumerReleaseSpec,
    op: &str,
    context: TapInvocationContext,
) -> Result<(), RenderError> {
    if spec.policy.channel == PackageReleaseChannel::Stable {
        push(env, "VELNOR_STABLE_OPERATION", "${{ inputs.operation }}");
        let stable = spec
            .stable
            .as_ref()
            .ok_or_else(|| RenderError::InvalidWorkflow("stable_policy_missing".to_owned()))?;
        push(
            env,
            "VELNOR_STABLE_VERSION_PACKAGE",
            &stable.version_package,
        );
        stable_bindings(env, op, context);
    }
    Ok(())
}
fn stable_bindings(env: &mut Vec<(String, Yaml)>, op: &str, context: TapInvocationContext) {
    if !matches!(context, TapInvocationContext::Stable) || op == "recover-package-release-v1" {
        return;
    }
    for (key, value) in [
        (
            "VELNOR_STABLE_TAG_ORIGIN_ARTIFACT_ID",
            "${{ needs.stable-tag.outputs.origin_artifact_id }}",
        ),
        (
            "VELNOR_STABLE_TAG_ORIGIN_ARTIFACT_DIGEST",
            "${{ needs.stable-tag.outputs.origin_artifact_digest }}",
        ),
        (
            "VELNOR_STABLE_TAG_INTENT_ARTIFACT_ID",
            "${{ needs.stable-tag.outputs.intent_artifact_id }}",
        ),
        (
            "VELNOR_STABLE_TAG_INTENT_ARTIFACT_DIGEST",
            "${{ needs.stable-tag.outputs.intent_artifact_digest }}",
        ),
    ] {
        push(env, key, value);
    }
    if matches!(
        op,
        "sign-package-release-v1"
            | "publish-package-release-v1"
            | "admit-release-aggregate-v1"
            | "acquire-release-leaves-v1"
    ) {
        push(
            env,
            "VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST",
            "${{ steps.origin-admission.outputs.admission_digest }}",
        );
    }
}
