use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Step, WorkflowIr};

use crate::{
    RenderError,
    lane_share::job_workflow_extract::{
        CALLER_INPUT_REQUIRED, CALLER_INPUT_TYPE, CALLER_INPUTS, JobWorkflowRelocation, PreparedJob,
    },
    render::RenderContext,
    steps,
    tree::RenderedFile,
    yaml::Yaml,
};

use super::{JobRenderPolicy, grants_exact_actions_read, job_to_yaml, permissions_to_yaml};

#[cfg(test)]
#[path = "job_workflow_tests.rs"]
mod tests;

pub(super) fn source_metadata_matches(
    prepared: &PreparedJob,
    emitted: &velnor_actions_contract::Job,
) -> bool {
    let source = &prepared.source_job;
    source.display_name == emitted.display_name
        && source.runs_on == emitted.runs_on
        && source.check_runner == emitted.check_runner
        && source.timeout_minutes == emitted.timeout_minutes
        && source.needs == emitted.needs
        && source.condition == emitted.condition
        && source.permissions == emitted.permissions
        && source.cache_mode == emitted.cache_mode
        && source.environment == emitted.environment
        && source.services == emitted.services
}

pub(super) fn caller_yaml(prepared: &PreparedJob) -> Result<Yaml, RenderError> {
    prepared.validate_source()?;
    let source = &prepared.source_job;
    steps::scan_for_private_subcommands(&source.display_name)?;
    let inputs = CALLER_INPUTS
        .iter()
        .map(|input| {
            steps::scan_for_private_subcommands(input.expression)?;
            Ok((input.name.to_owned(), Yaml::str(input.expression)))
        })
        .collect::<Result<Vec<_>, RenderError>>()?;
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(source.display_name.clone())),
        (
            "uses".to_owned(),
            Yaml::str(format!("./{}", prepared.callee_path)),
        ),
        ("with".to_owned(), Yaml::Map(inputs)),
    ];
    if let Some(permissions) = &source.permissions {
        fields.push(("permissions".to_owned(), permissions_to_yaml(permissions)));
    }
    if let Some(cache_mode) = &source.cache_mode {
        steps::scan_for_private_subcommands(cache_mode.as_str())?;
        fields.push(Yaml::cache_mode_field(cache_mode));
    }
    if !source.needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(source.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(condition) = &source.condition {
        steps::scan_for_private_subcommands(condition)?;
        fields.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    Ok(Yaml::Map(fields))
}

pub(super) fn relocation(
    prepared: &PreparedJob,
    ir: &WorkflowIr,
    ctx: &RenderContext,
    mbx_jobs: &BTreeSet<String>,
    workflow_env: &BTreeMap<String, String>,
) -> Result<JobWorkflowRelocation, RenderError> {
    prepared.validate_source()?;
    let document = callee_yaml(prepared, ir, ctx, mbx_jobs, workflow_env)?;
    let expected_callee_file = RenderedFile {
        path: prepared.callee_path.clone(),
        bytes: crate::render::fallback::render_checked_workflow(
            &prepared.callee_path,
            &document,
            &ctx.generator_version,
        )?,
    };
    steps::scan_for_private_subcommands(&expected_callee_file.bytes)?;
    crate::guard::validate_tree_path(&expected_callee_file.path)?;
    Ok(JobWorkflowRelocation {
        prepared: prepared.clone(),
        expected_caller: caller_yaml(prepared)?,
        expected_callee_file,
    })
}

fn callee_yaml(
    prepared: &PreparedJob,
    ir: &WorkflowIr,
    ctx: &RenderContext,
    mbx_jobs: &BTreeSet<String>,
    workflow_env: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    let job = &prepared.child_job;
    let actions_read =
        grants_exact_actions_read(job.permissions.as_ref().unwrap_or(&ir.permissions).actions);
    let empty_checkouts = BTreeMap::<String, Step>::new();
    let empty_steps = BTreeMap::<String, Vec<Step>>::new();
    let lanes = crate::document_lanes::SharedLaneSteps {
        checkouts: &empty_checkouts,
        env_steps: &empty_steps,
        runtime_preludes: &empty_steps,
        prefixes: &empty_steps,
        preludes: &empty_steps,
        postludes: &empty_steps,
    };
    let rendered_job = job_to_yaml(
        &prepared.source_id,
        job,
        ctx,
        &[],
        None,
        &lanes,
        JobRenderPolicy {
            native_mbx: mbx_jobs.contains(&prepared.source_id),
            plan_attempt: Some("${{ inputs.run_key_attempt }}"),
            actions_read,
            workflow_env,
        },
    )?;
    let mut top = vec![
        ("name".to_owned(), Yaml::str(ir.name.clone())),
        ("on".to_owned(), workflow_call_trigger()),
        (
            "permissions".to_owned(),
            permissions_to_yaml(&ir.permissions),
        ),
    ];
    if let Some(cache_mode) = &ir.cache_mode {
        steps::scan_for_private_subcommands(cache_mode.as_str())?;
        top.push(Yaml::cache_mode_field(cache_mode));
    }
    top.extend([
        (
            "env".to_owned(),
            crate::document_steps::string_map_yaml(workflow_env),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(prepared.source_id.clone(), rendered_job)]),
        ),
    ]);
    Ok(Yaml::Map(top))
}

fn workflow_call_trigger() -> Yaml {
    let inputs = CALLER_INPUTS
        .iter()
        .map(|input| {
            (
                input.name.to_owned(),
                Yaml::Map(vec![
                    ("type".to_owned(), Yaml::str(CALLER_INPUT_TYPE)),
                    ("required".to_owned(), Yaml::Bool(CALLER_INPUT_REQUIRED)),
                ]),
            )
        })
        .collect();
    Yaml::Map(vec![(
        "workflow_call".to_owned(),
        Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))]),
    )])
}
