// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepKind};

use crate::{
    RenderError, commands, guard,
    publish_spec::{PublishJobSpec, PublishStep, PublishTriggers, PublishWorkflowSpec},
    release_permissions::PermissionLevel,
    release_tree::ReleaseRenderContext,
    render::RenderedFile,
    step_ids::push_explicit_step_id,
    steps,
    yaml::Yaml,
};

pub const PUBLISH_WORKFLOW_PATH: &str = ".github/workflows/publish.yml";

pub const PUBLISH_CONCURRENCY_GROUP: &str = "publish-${{ github.ref }}";

pub fn render_publish_workflow(
    spec: &PublishWorkflowSpec,
    ctx: &ReleaseRenderContext,
) -> Result<RenderedFile, RenderError> {
    ctx.validate()?;
    spec.validate()?;
    for (id, job) in &spec.jobs {
        if job.runs_on != ctx.runs_on {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    let mut jobs = Vec::with_capacity(spec.jobs.len());
    for (id, job) in &spec.jobs {
        jobs.push((id.clone(), job_to_yaml(job, spec.staged_binary.as_deref())?));
    }
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(spec.name.clone())),
        ("on".to_owned(), triggers_to_yaml(&spec.triggers)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(PUBLISH_CONCURRENCY_GROUP)),
                ("queue".to_owned(), Yaml::str("max")),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]);
    let text = crate::render::fallback::render_checked_workflow(
        PUBLISH_WORKFLOW_PATH,
        &document,
        &ctx.generator_version,
    )?;
    steps::scan_for_private_subcommands(&text)?;
    guard::validate_allowlisted_path(PUBLISH_WORKFLOW_PATH, &[PUBLISH_WORKFLOW_PATH])?;
    Ok(RenderedFile {
        path: PUBLISH_WORKFLOW_PATH.to_owned(),
        bytes: text,
    })
}

fn triggers_to_yaml(triggers: &PublishTriggers) -> Yaml {
    let list = |items: &[String]| Yaml::Seq(items.iter().map(|i| Yaml::str(i.clone())).collect());
    let mut push = Vec::new();
    if !triggers.branches.is_empty() {
        push.push(("branches".to_owned(), list(&triggers.branches)));
    }
    if let Some(glob) = &triggers.tag_glob {
        push.push(("tags".to_owned(), list(std::slice::from_ref(glob))));
    }
    Yaml::Map(vec![("push".to_owned(), Yaml::Map(push))])
}

fn permissions_to_yaml(job: &PublishJobSpec) -> Yaml {
    let perms = &job.permissions;
    let rows = [
        ("contents", perms.contents),
        ("actions", perms.actions),
        ("checks", perms.checks),
        ("pull-requests", perms.pull_requests),
        ("attestations", perms.attestations),
        ("id-token", perms.id_token),
        ("packages", perms.packages),
    ];
    Yaml::Map(
        rows.iter()
            .filter(|(name, level)| *name == "contents" || *level != PermissionLevel::None)
            .map(|(name, level)| ((*name).to_owned(), Yaml::str(level.as_str())))
            .collect(),
    )
}

fn internal_step_to_yaml(step: &Step, staged: &str) -> Result<Yaml, RenderError> {
    let StepKind::Internal { operation, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "internal_step_required".to_owned(),
        ));
    };
    steps::split_internal_operation(operation)?;
    commands::validate_env(env)?;
    steps::scan_for_private_subcommands(&step.name)?;
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    if env.contains_key(steps::INTERNAL_OP_ENV) {
        return Err(RenderError::InvalidWorkflow(
            "duplicate_internal_env".to_owned(),
        ));
    }
    let mut vars = vec![(
        steps::INTERNAL_OP_ENV.to_owned(),
        Yaml::str(operation.clone()),
    )];
    vars.extend(env.iter().map(|(k, v)| (k.clone(), Yaml::str(v.clone()))));
    entries.push(("env".to_owned(), Yaml::Map(vars)));
    entries.push((
        "run".to_owned(),
        Yaml::str(commands::quote_run_arg(staged)?),
    ));
    Ok(Yaml::Map(entries))
}

fn step_to_yaml(
    step: &PublishStep,
    staged: Option<&str>,
    native: bool,
) -> Result<Yaml, RenderError> {
    let rendered = match (&step.step.kind, staged) {
        (StepKind::Internal { operation, env }, _)
            if native
                && matches!(
                    operation.as_str(),
                    "native-image-publish-v1" | "native-image-promote-v1"
                ) =>
        {
            crate::publish_native_operation::render(&step.step, operation, env)?
        }
        (StepKind::Internal { .. }, Some(staged)) => internal_step_to_yaml(&step.step, staged)?,
        _ => crate::steps_plain::plain_step_to_yaml(&step.step)?,
    };
    let (Some(id), Yaml::Map(mut entries)) = (&step.id, rendered.clone()) else {
        return Ok(rendered);
    };
    let mut with_id = Vec::new();
    push_explicit_step_id(&mut with_id, id);
    entries.splice(1..1, with_id);
    Ok(Yaml::Map(entries))
}

fn job_to_yaml(job: &PublishJobSpec, staged: Option<&str>) -> Result<Yaml, RenderError> {
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if let Some(environment) = &job.environment {
        entries.push(("environment".to_owned(), Yaml::str(environment.clone())));
    }
    let native_image = matches!(
        job.role,
        crate::publish_spec::PublishRole::NativeImagePublish
            | crate::publish_spec::PublishRole::NativeImagePromote
    );
    if native_image {
        entries.push((
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("${{ github.repository }}-native-image-writers"),
                ),
                ("queue".to_owned(), Yaml::str("max")),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ));
        entries.push((
            "env".to_owned(),
            Yaml::Map(vec![(
                "VELNOR_IMAGE_SOURCE_SHA".to_owned(),
                Yaml::str("${{ github.sha }}"),
            )]),
        ));
    }
    entries.push(("permissions".to_owned(), permissions_to_yaml(job)));
    if !job.needs.is_empty() {
        let needs = job.needs.iter().map(|n| Yaml::str(n.clone())).collect();
        entries.push(("needs".to_owned(), Yaml::Seq(needs)));
    }
    if let Some(condition) = &job.condition {
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    if !job.outputs.is_empty() {
        let outputs = job
            .outputs
            .iter()
            .map(|(name, value)| (name.clone(), Yaml::str(value.clone())))
            .collect();
        entries.push(("outputs".to_owned(), Yaml::Map(outputs)));
    }
    let mut rendered = Vec::with_capacity(job.steps.len());
    for (index, step) in job.steps.iter().enumerate() {
        rendered.push(step_to_yaml(step, staged, native_image)?);
        if native_image && index == 0 {
            rendered.push(crate::schema2::qualified_sdk_preview_acquire());
        }
    }
    crate::document_services::push_services(&mut entries, &job.services);
    entries.push(("steps".to_owned(), Yaml::Seq(rendered)));
    Ok(Yaml::Map(entries))
}
