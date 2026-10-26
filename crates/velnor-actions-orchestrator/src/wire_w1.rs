use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_actionlint::{
    JactionlintCapabilities, PinnedActionRef, StepSyntax,
    actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION},
    checkout_inputs_schema, validate_action_inputs,
};
use velnor_actions_contract::{Job, ProposedTask, Step, StepKind, StepRole, VelnorConfig};
use velnor_actions_mise::{Gate6Fixture, TaskCacheMode, ToolCatalog, ToolHomes};
use velnor_actions_rust::is_workspace_fmt_task;
use velnor_actions_workflow_renderer::plan_format;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, TASK_ARTIFACTS_DIR, cache_action_step, check_mbx_gating,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::utf8::strings_of_env;

#[must_use]
pub(crate) fn declared_config_variables(config: &VelnorConfig) -> Vec<String> {
    let mut names = BTreeSet::new();
    if let Some(completion) = &config.workflow.review_completion {
        names.insert(completion.app_id_variable.clone());
        names.insert(completion.reviewer_app_id_variable.clone());
    }
    if let Some(private) = &config.private_sources {
        names.insert(private.app_id_variable.clone());
    }
    if let Some(upgrades) = config.upgrades.as_ref().filter(|upgrades| upgrades.enabled) {
        names.insert(upgrades.app_id_variable.clone());
    }
    if let Some(tap) = config
        .distribution
        .homebrew_tap
        .as_ref()
        .filter(|tap| tap.enabled)
    {
        names.insert(tap.app_id_variable.clone());
        names.insert(tap.protection_observer.app_id_variable.clone());
        names.insert(tap.review_completion.app_id_variable.clone());
        names.extend(
            tap.review_completion
                .expected_reviewers
                .iter()
                .map(|reviewer| reviewer.app_id_variable.clone()),
        );
    }
    names.into_iter().collect()
}

pub(crate) fn checkout_step() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    validate_action_inputs(&checkout_inputs_schema(), &with).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        parallel: false,
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
            env: BTreeMap::new(),
        },
    })
}

pub(crate) fn apply_checkout_lfs(jobs: &mut BTreeMap<String, Job>) {
    for step in jobs.values_mut().flat_map(|job| job.steps.iter_mut()) {
        if step.role != Some(StepRole::Checkout) {
            continue;
        }
        if let StepKind::Action { with, .. } = &mut step.kind {
            with.insert("lfs".to_owned(), "true".to_owned());
        }
    }
}

pub(crate) fn checkout_step_full() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("fetch-depth".to_owned(), "0".to_owned()),
    ]);
    validate_action_inputs(&checkout_inputs_schema(), &with).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        parallel: false,
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
            env: BTreeMap::new(),
        },
    })
}

pub(crate) fn vet_step_syntax(syntax: StepSyntax) -> Result<(), OrchestratorError> {
    JactionlintCapabilities::for_pinned()
        .check_step_syntax(syntax)
        .map_err(OrchestratorError::from)
}

pub(crate) fn maybe_task_cache_steps(
    fixture: Option<&Gate6Fixture>,
    mode: TaskCacheMode,
    key: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    if fixture.is_none() || mode == TaskCacheMode::Off {
        return Ok(Vec::new());
    }
    let restore_uses = PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value();
    let save_uses = PinnedActionRef::new(
        "actions/cache",
        Some("save"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value();
    let paths = vec![TASK_ARTIFACTS_DIR.to_owned()];
    Ok(vec![
        cache_action_step(true, &restore_uses, "task", key, &[], &paths)?,
        cache_action_step(false, &save_uses, "task", key, &[], &paths)?,
    ])
}

pub(crate) const REPORT_FORMAT_NAME: &str = "Report Format";

pub(crate) fn workspace_format_step(
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Option<Step>, OrchestratorError> {
    let Some(fmt) = workspace_fmt_group(discovery) else {
        return Ok(None);
    };
    let argv = crate::vectors::task_argv(fmt, catalog)?;
    if argv.first().is_none_or(|program| program != "mise") {
        return Err(OrchestratorError::Contract {
            problem: "format_without_mise".to_owned(),
        });
    }
    let joined = velnor_actions_workflow_renderer::join_argv_for_run(&argv).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let matrix_key = matrix_key_for(fmt)?;
    let outcome = crate::matrix_step::outcome_path_for_key(&matrix_key);
    let start = crate::matrix_step::start_path_for_key(&matrix_key);
    let run = crate::matrix_step::outcome_wrapper_argv(&joined, &outcome, &start);
    let env = format_step_env(catalog)?;
    plan_format::format_step(run, &env)
        .map(Some)
        .map_err(OrchestratorError::from)
}

pub(crate) fn workspace_format_report_steps(
    discovery: &Discovery,
    helper_version: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    let Some(fmt) = workspace_fmt_group(discovery) else {
        return Ok(Vec::new());
    };
    let matrix_key = matrix_key_for(fmt)?;
    let outcome = crate::matrix_step::outcome_path_for_key(&matrix_key);
    let start = crate::matrix_step::start_path_for_key(&matrix_key);
    let helper = crate::matrix_step::helper_path_for_version(helper_version);
    let report = velnor_actions_workflow_renderer::shell_step(
        REPORT_FORMAT_NAME,
        crate::matrix_step::deferred_report_argv(&outcome, &helper, &start),
        BTreeMap::from([(
            crate::task_report::TASK_ID_ENV.to_owned(),
            fmt.task_id.clone(),
        )]),
    )
    .map_err(OrchestratorError::from)?;
    let upload = crate::matrix_step::crate_upload_step(PLAN_JOB_ID)?;
    Ok(vec![report, upload])
}

fn workspace_fmt_group(discovery: &Discovery) -> Option<&ProposedTask> {
    discovery.proposals.iter().find(|task| {
        is_workspace_fmt_task(&task.task_kind, &task.identity.unit_id, &task.display_name)
    })
}

fn matrix_key_for(task: &ProposedTask) -> Result<String, OrchestratorError> {
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group(&task.stack_id, &task.task_id)
            .map_err(crate::internal::internal_contract)?;
    velnor_actions_contract::matrix_key_for_id(&matrix_id)
        .map_err(crate::internal::internal_contract)
}

fn format_step_env(catalog: &ToolCatalog) -> Result<BTreeMap<String, String>, OrchestratorError> {
    strings_of_env(&ToolHomes::runner_temp().exec_env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })
}

pub(crate) fn check_crate_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), OrchestratorError> {
    check_mbx_gating(jobs, drivers).map_err(OrchestratorError::from)
}
