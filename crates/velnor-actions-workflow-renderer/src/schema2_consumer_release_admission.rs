use super::{
    ConsumerReleaseSpec, PackageReleaseChannel, RenderError, TapInvocationContext, Yaml, operation,
    production,
};
pub(super) fn policy_admission(spec: &ConsumerReleaseSpec) -> Result<Yaml, RenderError> {
    let mut step = operation(
        spec,
        match spec.policy.channel {
            PackageReleaseChannel::Preview => "acquire-preview-source-v1",
            PackageReleaseChannel::Stable => "admit-stable-tag-origin-v1",
        },
        true,
        TapInvocationContext::for_channel(spec.policy.channel),
    )?;
    if let Yaml::Map(fields) = &mut step
        && let Some((_, id)) = fields.iter_mut().find(|(key, _)| key == "id")
    {
        *id = Yaml::str(match spec.policy.channel {
            PackageReleaseChannel::Preview => "policy-admission",
            PackageReleaseChannel::Stable => "origin-admission",
        });
    }
    Ok(step)
}

pub(super) fn aggregate_admission(spec: &ConsumerReleaseSpec) -> Result<Yaml, RenderError> {
    let mut step = operation(
        spec,
        "admit-release-aggregate-v1",
        true,
        TapInvocationContext::for_channel(spec.policy.channel),
    )?;
    if let Yaml::Map(fields) = &mut step {
        let (_, id) = fields
            .iter_mut()
            .find(|(key, _)| key == "id")
            .ok_or_else(|| {
                RenderError::InvalidWorkflow("preview_operation_id_missing".to_owned())
            })?;
        *id = Yaml::str("aggregate");
    }
    production::operation_env(
        &mut step,
        &[
            (
                "VELNOR_RELEASE_AGGREGATE_ARTIFACT_ID",
                match spec.policy.channel {
                    PackageReleaseChannel::Preview => {
                        "${{ needs.preview-assemble.outputs.artifact_id }}"
                    }
                    PackageReleaseChannel::Stable => {
                        "${{ needs.stable-assemble.outputs.artifact_id }}"
                    }
                },
            ),
            (
                "VELNOR_RELEASE_AGGREGATE_ARTIFACT_DIGEST",
                match spec.policy.channel {
                    PackageReleaseChannel::Preview => {
                        "${{ needs.preview-assemble.outputs.artifact_digest }}"
                    }
                    PackageReleaseChannel::Stable => {
                        "${{ needs.stable-assemble.outputs.artifact_digest }}"
                    }
                },
            ),
        ],
    )?;
    Ok(step)
}

pub(super) fn finish_policy_job(
    id: &str,
    mut fields: Vec<(String, Yaml)>,
    mut steps: Vec<Yaml>,
) -> Result<(String, Yaml), RenderError> {
    let mut policy = None;
    for step in &mut steps {
        let has_policy = matches!(step, Yaml::Map(fields) if fields.iter().any(|(key, value)| key == "env" && matches!(value, Yaml::Map(env) if env.iter().any(|(key, _)| key == "VELNOR_RELEASE_POLICY"))));
        if !has_policy {
            continue;
        }
        let current = production::take_policy_environment(step)?;
        if policy.as_ref().is_some_and(|expected| expected != &current) {
            return Err(RenderError::InvalidWorkflow(
                "release_job_policy_mismatch".to_owned(),
            ));
        }
        policy = Some(current);
    }
    fields.push((
        "env".to_owned(),
        policy
            .ok_or_else(|| RenderError::InvalidWorkflow("release_job_policy_missing".to_owned()))?,
    ));
    Ok(super::finish(id, fields, steps))
}
