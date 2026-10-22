//! Literal payload jobs and complete-union assembly without signing authority.

use super::{
    ConsumerReleaseSpec, JobTimeout, PackageReleaseChannel, RenderError, Yaml, acquire, base,
    finish, join_argv_for_run, non_internal_step_to_yaml, operation, permissions, run_step, setup,
    steps,
};
use velnor_actions_contract::preview_production::ReleasePayloadPair;

#[path = "schema2_consumer_preview_production_acquisition.rs"]
mod acquisition;
use acquisition::{acquisition_steps, source_steps};

pub(in crate::schema2) fn jobs(
    spec: &ConsumerReleaseSpec,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    if spec.policy.channel == PackageReleaseChannel::Preview {
        return Ok(vec![
            producer(
                spec,
                "preview-payload",
                None,
                &["preview-eligibility".to_owned()],
                true,
            )?,
            producer(
                spec,
                "preview-assemble",
                None,
                &[
                    "preview-eligibility".to_owned(),
                    "preview-payload".to_owned(),
                ],
                false,
            )?,
        ]);
    }
    let pairs = spec
        .policy
        .payload_pairs()
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut jobs = Vec::with_capacity(pairs.len() + 1);
    let context = super::TapInvocationContext::for_channel(spec.policy.channel);
    let mut dependencies = vec![context.selection().to_owned()];
    for pair in &pairs {
        let id = spec
            .policy
            .payload_job_id(pair)
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
        dependencies.push(id.clone());
        jobs.push(producer(
            spec,
            &id,
            Some(pair),
            &[context.selection().to_owned()],
            false,
        )?);
    }
    jobs.push(producer(
        spec,
        &format!("{}-assemble", context.prefix()),
        None,
        &dependencies,
        false,
    )?);
    Ok(jobs)
}

fn producer(
    spec: &ConsumerReleaseSpec,
    id: &str,
    pair: Option<&ReleasePayloadPair>,
    dependencies: &[String],
    preview_matrix: bool,
) -> Result<(String, Yaml), RenderError> {
    // Assembly is an authenticated service jobs API witness and keeps its closed
    // name. Payload display names describe the pair without changing machine IDs.
    let name = if preview_matrix {
        "Build ${{ matrix.product_id }} / ${{ matrix.target }}".to_owned()
    } else {
        pair.map_or_else(
            || id.to_owned(),
            |pair| format!("Build {} / {}", pair.product_id, pair.target),
        )
    };
    let runs_on = if preview_matrix {
        "${{ matrix.runner }}"
    } else if spec.policy.channel == PackageReleaseChannel::Preview {
        "${{ needs.preview-eligibility.outputs.producer_runner }}"
    } else {
        spec.producer_runner.runs_on()
    };
    let job_timeout = if spec.policy.channel == PackageReleaseChannel::Preview {
        JobTimeout::DEFAULT
    } else {
        JobTimeout::new(
            spec.producer_timeout_minutes
                + velnor_actions_contract::config::RustBinaryReleaseProducer::JOB_HEADROOM_MINUTES,
        )
        .map_err(RenderError::Contract)?
    };
    let mut fields = base(&name, Yaml::str(runs_on), job_timeout);
    if spec.policy.channel == PackageReleaseChannel::Preview {
        bind_preview_runtime_fields(&mut fields, preview_matrix);
    }
    fields.push((
        "if".to_owned(),
        Yaml::str(super::TapInvocationContext::for_channel(spec.policy.channel).production_if()),
    ));
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(dependencies.iter().map(Yaml::str).collect()),
    ));
    if preview_matrix {
        fields.push((
            "strategy".to_owned(),
            Yaml::Map(vec![
                ("fail-fast".to_owned(), Yaml::Bool(false)),
                (
                    "matrix".to_owned(),
                    Yaml::str("${{ fromJSON(needs.preview-eligibility.outputs.payload_matrix) }}"),
                ),
            ]),
        ));
    }
    let access = vec![("actions", "read"), ("contents", "read")];
    fields.push(("permissions".to_owned(), permissions(&access)));
    fields.push((
        "outputs".to_owned(),
        permissions(&[
            ("artifact_id", "${{ steps.upload.outputs.artifact-id }}"),
            (
                "artifact_digest",
                "${{ steps.upload.outputs.artifact-digest }}",
            ),
        ]),
    ));
    let mut steps = acquisition_steps(spec, id, pair.is_none() && !preview_matrix)?;
    steps.extend(source_steps(spec, id)?);
    let (mut operation, public_plan) =
        production_operation(spec, "execute-release-producer-v1", false)?;
    // The public full plan is identical for every production owner. Keeping it
    // at job scope lets existing workflow mapping sharing represent it once,
    // without changing the literal stage/pair or per-operation source bindings.
    fields.push(("env".to_owned(), public_plan));
    bind_stage(&mut operation, spec, pair, preview_matrix)?;
    steps.push(operation);
    steps.push(upload(pair, preview_matrix));
    for step in &mut steps {
        super::steps::with_producer_homes(step, &spec.producer_cargo_env)?;
    }
    Ok(finish(id, fields, steps))
}

fn bind_preview_runtime_fields(fields: &mut Vec<(String, Yaml)>, preview_matrix: bool) {
    // The runner is selected from authenticated source policy at runtime.
    // Both admitted Linux and macOS hosted runners provide Bash, so make
    // that default explicit for static shell validation as well.
    fields.push((
        "defaults".to_owned(),
        Yaml::Map(vec![(
            "run".to_owned(),
            Yaml::Map(vec![("shell".to_owned(), Yaml::str("bash"))]),
        )]),
    ));
    let timeout = if preview_matrix {
        "${{ matrix.timeout_minutes }}"
    } else {
        "${{ fromJSON(needs.preview-eligibility.outputs.producer_timeout_minutes) }}"
    };
    if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "timeout-minutes") {
        *value = Yaml::str(timeout);
    }
}

fn bind_stage(
    operation: &mut Yaml,
    spec: &ConsumerReleaseSpec,
    pair: Option<&ReleasePayloadPair>,
    preview_matrix: bool,
) -> Result<(), RenderError> {
    let env = if preview_matrix {
        vec![
            ("VELNOR_RELEASE_STAGE", "payload"),
            ("VELNOR_RELEASE_PRODUCT_ID", "${{ matrix.product_id }}"),
            ("VELNOR_RELEASE_TARGET", "${{ matrix.target }}"),
        ]
    } else if let Some(pair) = pair {
        vec![
            ("VELNOR_RELEASE_STAGE", "payload"),
            ("VELNOR_RELEASE_PRODUCT_ID", pair.product_id.as_str()),
            ("VELNOR_RELEASE_TARGET", pair.target.as_str()),
        ]
    } else if spec.policy.channel == PackageReleaseChannel::Preview {
        vec![("VELNOR_RELEASE_STAGE", "assemble")]
    } else {
        vec![
            ("VELNOR_RELEASE_STAGE", "assemble"),
            ("VELNOR_RELEASE_NEEDS_JSON", "${{ toJSON(needs) }}"),
        ]
    };
    operation_env(operation, &env)?;
    if spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Stable
    {
        operation_env(
            operation,
            &[(
                "VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST",
                "${{ steps.origin-admission.outputs.admission_digest }}",
            )],
        )?;
    }
    if spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
    {
        operation_env(
            operation,
            &[(
                "VELNOR_RELEASE_POLICY_ADMISSION_DIGEST",
                "${{ steps.policy-admission.outputs.admission_digest }}",
            )],
        )?;
    }
    if pair.is_none() && spec.policy.channel == PackageReleaseChannel::Stable {
        operation_env(
            operation,
            &[(
                "VELNOR_RELEASE_ADMISSION_DIGEST",
                "${{ steps.acquire.outputs.admission_digest }}",
            )],
        )?;
    }
    Ok(())
}

fn set_id(step: &mut Yaml, id: &str) {
    if let Yaml::Map(fields) = step
        && let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "id")
    {
        *value = Yaml::str(id);
    }
}

/// Every native production operation inherits the identical public job plan.
/// Credentials and admission outputs stay in their existing step-local boundary.
fn production_operation(
    spec: &ConsumerReleaseSpec,
    op: &str,
    authenticated: bool,
) -> Result<(Yaml, Yaml), RenderError> {
    let mut step = operation(
        spec,
        op,
        authenticated,
        super::TapInvocationContext::for_channel(spec.policy.channel),
    )?;
    let public_plan = take_policy_environment(&mut step)?;
    Ok((step, public_plan))
}

pub(super) fn take_policy_environment(step: &mut Yaml) -> Result<Yaml, RenderError> {
    let Yaml::Map(fields) = step else {
        return Err(RenderError::InvalidWorkflow(
            "preview_operation_not_map".to_owned(),
        ));
    };
    let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
        return Err(RenderError::InvalidWorkflow(
            "preview_operation_env_missing".to_owned(),
        ));
    };
    if env
        .iter()
        .filter(|(key, _)| key == "VELNOR_RELEASE_POLICY")
        .count()
        != 1
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_operation_plan_count".to_owned(),
        ));
    }
    let index = env
        .iter()
        .position(|(key, _)| key == "VELNOR_RELEASE_POLICY")
        .ok_or_else(|| RenderError::InvalidWorkflow("preview_operation_plan_missing".to_owned()))?;
    Ok(Yaml::Map(vec![env.remove(index)]))
}

pub(in crate::schema2) fn operation_env(
    step: &mut Yaml,
    values: &[(&str, &str)],
) -> Result<(), RenderError> {
    let Yaml::Map(fields) = step else {
        return Err(RenderError::InvalidWorkflow(
            "preview_operation_not_map".to_owned(),
        ));
    };
    let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
        return Err(RenderError::InvalidWorkflow(
            "preview_operation_env_missing".to_owned(),
        ));
    };
    for (key, value) in values {
        if env.iter().any(|(existing, _)| existing == key) {
            return Err(RenderError::InvalidWorkflow(
                "preview_operation_env_conflict".to_owned(),
            ));
        }
        env.push(((*key).to_owned(), Yaml::str(*value)));
    }
    Ok(())
}

pub(in crate::schema2) fn upload(pair: Option<&ReleasePayloadPair>, preview_matrix: bool) -> Yaml {
    let prefix = "${{ github.run_id }}-a${{ github.run_attempt }}";
    let name = if preview_matrix {
        format!(
            "velnor-release-payload-r{prefix}-${{{{ matrix.product_id }}}}-${{{{ matrix.target }}}}"
        )
    } else {
        pair.map_or_else(
            || format!("velnor-release-aggregate-r{prefix}"),
            |pair| {
                format!(
                    "velnor-release-payload-r{prefix}-{}-{}",
                    pair.product_id, pair.target
                )
            },
        )
    };
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Retain immutable native production evidence"),
        ),
        ("id".to_owned(), Yaml::str("upload")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(name)),
                (
                    "path".to_owned(),
                    Yaml::str("${{ runner.temp }}/velnor/release-output"),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(30)),
                ("overwrite".to_owned(), Yaml::Bool(false)),
                ("include-hidden-files".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}
