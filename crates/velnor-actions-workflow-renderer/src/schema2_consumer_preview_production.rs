//! Literal payload jobs and complete-union assembly without signing authority.

use super::{
    BTreeMap, ConsumerPreviewSpec, JobTimeout, RenderError, Yaml, acquire, base, finish,
    join_argv_for_run, non_internal_step_to_yaml, operation, permissions, run_step, setup, steps,
};
use velnor_actions_contract::preview_production::{
    PreviewPayloadPair, expected_preview_payload_pairs, preview_payload_job_id,
};

pub(super) fn jobs(spec: &ConsumerPreviewSpec) -> Result<Vec<(String, Yaml)>, RenderError> {
    let pairs = expected_preview_payload_pairs(&spec.plan)
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut jobs = Vec::with_capacity(pairs.len() + 1);
    let mut dependencies = vec!["preview-eligibility".to_owned()];
    for pair in &pairs {
        let id = preview_payload_job_id(&spec.plan, pair)
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
        dependencies.push(id.clone());
        jobs.push(producer(
            spec,
            &id,
            Some(pair),
            &["preview-eligibility".to_owned()],
        )?);
    }
    jobs.push(producer(spec, "preview-assemble", None, &dependencies)?);
    Ok(jobs)
}

fn producer(
    spec: &ConsumerPreviewSpec,
    id: &str,
    pair: Option<&PreviewPayloadPair>,
    dependencies: &[String],
) -> Result<(String, Yaml), RenderError> {
    // The service jobs API exposes the display name, so it must equal the closed job ID.
    let mut fields = base(
        id,
        Yaml::str(spec.producer_runner.runs_on()),
        JobTimeout::new(spec.producer_timeout_minutes).map_err(RenderError::Contract)?,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str("${{ github.event_name == 'workflow_run' }}"),
    ));
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(dependencies.iter().map(Yaml::str).collect()),
    ));
    let access = if pair.is_some() {
        vec![("contents", "read")]
    } else {
        vec![("actions", "read"), ("contents", "read")]
    };
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
    steps.extend(source_steps(spec, id, pair.is_some())?);
    let (mut operation, public_plan) =
        production_operation(spec, "execute-preview-producer-v1", false)?;
    // The public full plan is identical for every production owner. Keeping it
    // at job scope lets existing workflow mapping sharing represent it once,
    // without changing the literal stage/pair or per-operation source bindings.
    fields.push(("env".to_owned(), public_plan));
    let env = if let Some(pair) = pair {
        vec![
            ("VELNOR_PREVIEW_STAGE", "payload"),
            ("VELNOR_PREVIEW_PRODUCT_ID", pair.product_id.as_str()),
            ("VELNOR_PREVIEW_TARGET", pair.target.as_str()),
        ]
    } else {
        vec![
            ("VELNOR_PREVIEW_STAGE", "assemble"),
            ("VELNOR_PREVIEW_NEEDS_JSON", "${{ toJSON(needs) }}"),
        ]
    };
    operation_env(&mut operation, &env)?;
    if pair.is_none() {
        reuse_admission_plan(&mut operation)?;
        operation_env(
            &mut operation,
            &[(
                "VELNOR_PREVIEW_ADMISSION_DIGEST",
                "${{ steps.acquire.outputs.admission_digest }}",
            )],
        )?;
    }
    steps.push(operation);
    steps.push(upload(pair));
    for step in &mut steps {
        super::steps::with_producer_homes(step, &spec.producer_cargo_env)?;
    }
    Ok(finish(id, fields, steps))
}

fn acquisition_steps(
    spec: &ConsumerPreviewSpec,
    id: &str,
    needed: bool,
) -> Result<Vec<Yaml>, RenderError> {
    if !needed {
        return Ok(Vec::new());
    }
    // Only pinned source-free bootstrap precedes acquisition; no checkout,
    // Mise task or repository preparation runs with this credential.
    let (mut admission, _) = production_operation(spec, "acquire-preview-leaves-v1", true)?;
    if let Yaml::Map(fields) = &mut admission {
        for (key, value) in fields {
            if key == "id" {
                *value = Yaml::str("acquire");
            }
        }
    }
    operation_env(
        &mut admission,
        &[
            ("VELNOR_PREVIEW_STAGE", "assemble"),
            ("VELNOR_PREVIEW_NEEDS_JSON", "${{ toJSON(needs) }}"),
        ],
    )?;
    Ok(vec![
        setup(&spec.producer_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.producer_acquire, id, spec.producer_runner.runs_on())?,
        admission,
    ])
}

fn reuse_admission_plan(operation: &mut Yaml) -> Result<(), RenderError> {
    let materialize = "(set -C; printf '%s' \"$VELNOR_RELEASE_PLAN\" > \"$VELNOR_REQUEST_FILE\")\n";
    if let Yaml::Map(fields) = operation {
        for (key, value) in fields {
            if key == "run"
                && let Yaml::Str(run) = value
                && run.matches(materialize).count() == 1
            {
                *run = run.replace(materialize, "");
                return Ok(());
            }
        }
    }
    Err(RenderError::InvalidWorkflow(
        "preview_admission_plan_materialization".to_owned(),
    ))
}

/// Every native production operation inherits the identical public job plan.
/// Credentials and admission outputs stay in their existing step-local boundary.
fn production_operation(
    spec: &ConsumerPreviewSpec,
    op: &str,
    authenticated: bool,
) -> Result<(Yaml, Yaml), RenderError> {
    let mut step = operation(spec, op, authenticated)?;
    let public_plan = take_plan_environment(&mut step)?;
    Ok((step, public_plan))
}

fn take_plan_environment(step: &mut Yaml) -> Result<Yaml, RenderError> {
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
        .filter(|(key, _)| key == "VELNOR_RELEASE_PLAN")
        .count()
        != 1
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_operation_plan_count".to_owned(),
        ));
    }
    let index = env
        .iter()
        .position(|(key, _)| key == "VELNOR_RELEASE_PLAN")
        .ok_or_else(|| RenderError::InvalidWorkflow("preview_operation_plan_missing".to_owned()))?;
    Ok(Yaml::Map(vec![env.remove(index)]))
}

pub(super) fn operation_env(step: &mut Yaml, values: &[(&str, &str)]) -> Result<(), RenderError> {
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

pub(super) fn upload(pair: Option<&PreviewPayloadPair>) -> Yaml {
    let prefix = "${{ github.run_id }}-a${{ github.run_attempt }}";
    let name = pair.map_or_else(
        || format!("velnor-preview-aggregate-r{prefix}"),
        |pair| {
            format!(
                "velnor-preview-payload-r{prefix}-{}-{}",
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
                    Yaml::str("${{ runner.temp }}/velnor/preview-output"),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(30)),
                ("overwrite".to_owned(), Yaml::Bool(false)),
                ("include-hidden-files".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

fn source_steps(
    spec: &ConsumerPreviewSpec,
    id: &str,
    acquire_generator: bool,
) -> Result<Vec<Yaml>, RenderError> {
    let runner = spec.producer_runner.runs_on();
    crate::toolchain_env::check_toolchain_homes(&spec.producer_cargo_env)?;
    crate::toolchain_env::reject_denied_step_keys(&spec.producer_cargo_env)?;
    let checkout = steps::checkout("${{ needs.preview-eligibility.outputs.source_sha }}");
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
    ];
    if acquire_generator {
        producer_steps.insert(1, setup(&spec.producer_setup));
    }
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
    if acquire_generator {
        producer_steps.push(acquire(&spec.producer_acquire, id, runner)?);
    }
    Ok(producer_steps)
}
