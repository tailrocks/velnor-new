//! Literal payload jobs and complete-union assembly without signing authority.

use super::{
    BTreeMap, ConsumerReleaseSpec, JobTimeout, RenderError, Yaml, acquire, base, finish,
    join_argv_for_run, non_internal_step_to_yaml, operation, permissions, run_step, setup, steps,
};
use velnor_actions_contract::preview_production::ReleasePayloadPair;

pub(in crate::schema2) fn jobs(
    spec: &ConsumerReleaseSpec,
) -> Result<Vec<(String, Yaml)>, RenderError> {
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
        )?);
    }
    jobs.push(producer(
        spec,
        &format!("{}-assemble", context.prefix()),
        None,
        &dependencies,
    )?);
    Ok(jobs)
}

fn producer(
    spec: &ConsumerReleaseSpec,
    id: &str,
    pair: Option<&ReleasePayloadPair>,
    dependencies: &[String],
) -> Result<(String, Yaml), RenderError> {
    // Assembly is an authenticated service jobs API witness and keeps its closed
    // name. Payload display names describe the pair without changing machine IDs.
    let name = pair.map_or_else(
        || id.to_owned(),
        |pair| format!("Build {} / {}", pair.product_id, pair.target),
    );
    let mut fields = base(
        &name,
        Yaml::str(spec.producer_runner.runs_on()),
        JobTimeout::new(
            spec.producer_timeout_minutes
                + velnor_actions_contract::config::RustBinaryReleaseProducer::JOB_HEADROOM_MINUTES,
        )
        .map_err(RenderError::Contract)?,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str(super::TapInvocationContext::for_channel(spec.policy.channel).production_if()),
    ));
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(dependencies.iter().map(Yaml::str).collect()),
    ));
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
    let mut steps = acquisition_steps(spec, id, pair.is_none())?;
    steps.extend(source_steps(spec, id)?);
    let (mut operation, public_plan) =
        production_operation(spec, "execute-release-producer-v1", false)?;
    // The public full plan is identical for every production owner. Keeping it
    // at job scope lets existing workflow mapping sharing represent it once,
    // without changing the literal stage/pair or per-operation source bindings.
    fields.push(("env".to_owned(), public_plan));
    bind_stage(&mut operation, spec, pair)?;
    steps.push(operation);
    steps.push(upload(pair));
    for step in &mut steps {
        super::steps::with_producer_homes(step, &spec.producer_cargo_env)?;
    }
    Ok(finish(id, fields, steps))
}

fn bind_stage(
    operation: &mut Yaml,
    spec: &ConsumerReleaseSpec,
    pair: Option<&ReleasePayloadPair>,
) -> Result<(), RenderError> {
    let env = if let Some(pair) = pair {
        vec![
            ("VELNOR_RELEASE_STAGE", "payload"),
            ("VELNOR_RELEASE_PRODUCT_ID", pair.product_id.as_str()),
            ("VELNOR_RELEASE_TARGET", pair.target.as_str()),
        ]
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
    if pair.is_none() {
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

fn acquisition_steps(
    spec: &ConsumerReleaseSpec,
    id: &str,
    needed: bool,
) -> Result<Vec<Yaml>, RenderError> {
    let stable = spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Stable;
    let mut result = vec![
        setup(&spec.producer_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.producer_acquire, id, spec.producer_runner.runs_on())?,
    ];
    if stable {
        let (mut origin, _) = production_operation(spec, "admit-stable-tag-origin-v1", true)?;
        set_id(&mut origin, "origin-admission");
        result.push(origin);
    } else {
        let (mut admission, _) = production_operation(spec, "admit-release-policy-v1", true)?;
        set_id(&mut admission, "policy-admission");
        result.push(admission);
    }
    if needed {
        let (mut admission, _) = production_operation(spec, "acquire-release-leaves-v1", true)?;
        set_id(&mut admission, "acquire");
        operation_env(
            &mut admission,
            &[
                ("VELNOR_RELEASE_STAGE", "assemble"),
                ("VELNOR_RELEASE_NEEDS_JSON", "${{ toJSON(needs) }}"),
            ],
        )?;
        result.push(admission);
    }
    Ok(result)
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

pub(in crate::schema2) fn upload(pair: Option<&ReleasePayloadPair>) -> Yaml {
    let prefix = "${{ github.run_id }}-a${{ github.run_attempt }}";
    let name = pair.map_or_else(
        || format!("velnor-release-aggregate-r{prefix}"),
        |pair| {
            format!(
                "velnor-release-payload-r{prefix}-{}-{}",
                pair.product_id, pair.target
            )
        },
    );
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

fn source_steps(spec: &ConsumerReleaseSpec, id: &str) -> Result<Vec<Yaml>, RenderError> {
    let runner = spec.producer_runner.runs_on();
    crate::toolchain_env::check_toolchain_homes(&spec.producer_cargo_env)?;
    crate::toolchain_env::reject_denied_step_keys(&spec.producer_cargo_env)?;
    let checkout = steps::checkout(
        spec,
        super::TapInvocationContext::for_channel(spec.policy.channel).source(),
    );
    let mut producer_steps = vec![
        checkout,
        run_step(
            "Create owned Cargo metadata homes",
            "mkdir -p \"$MISE_CARGO_HOME\" \"$MISE_RUSTUP_HOME\"",
        ),
        run_step(
            "Install pinned Cargo metadata toolchain",
            &join_argv_for_run(&spec.install_rust_argv)?,
        ),
        run_step(
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            &join_argv_for_run(&spec.verify_rust_argv)?,
        ),
    ];
    for step in &spec.producer_source_prep {
        let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(
                "preview_source_preparation_not_shell".to_owned(),
            ));
        };
        if env != &spec.producer_cargo_env {
            return Err(RenderError::InvalidWorkflow(
                "preview_source_preparation_home_mismatch".to_owned(),
            ));
        }
        producer_steps.push(non_internal_step_to_yaml(
            id,
            step,
            &BTreeMap::new(),
            runner,
        )?);
    }
    Ok(producer_steps)
}
