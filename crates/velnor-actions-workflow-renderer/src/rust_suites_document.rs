//! Standalone YAML workflow document builder for declared Rust test suites.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    DEFAULT_SUITE_WORKFLOW_SLUG, Job, PermissionLevel, RunsOn, RustSuiteConfig, StepKind,
    WorkflowDispatchConfig,
};

use super::{RustSuitesRenderSpec, build_suite_jobs_for_entry};
use crate::{
    RenderError,
    document_steps::{non_internal_step_to_yaml, string_map_yaml},
    matrix,
    render::fallback::render_checked_workflow,
    steps,
    yaml::Yaml,
};

pub(super) fn render_workflow_for_slug(
    slug: &str,
    entries: &[&RustSuiteConfig],
    spec: &RustSuitesRenderSpec,
) -> Result<String, RenderError> {
    let mut jobs = BTreeMap::new();
    let mut allow_failure_jobs = BTreeSet::new();
    for suite in entries {
        let condition = suite_job_condition(suite);
        if suite.allow_failure {
            allow_failure_jobs.insert(format!("suite-{}", suite.id));
        }
        for (job_id, mut job) in build_suite_jobs_for_entry(suite, spec, true)? {
            job.condition.clone_from(&condition);
            jobs.insert(job_id, job);
        }
    }
    finalize_suite_workflow_jobs(&mut jobs, spec)?;
    let suite_matrices = matrix::suites::suite_matrices_of(&jobs)?;
    let scrubbed = if suite_matrices.is_empty() {
        jobs
    } else {
        matrix::scrub_matrix_marker(&jobs)
    };
    let mbx_jobs = crate::mbx_job_env::jobs_with_mbx(&scrubbed);
    let workflow_env = crate::toolchain_env::workflow_level_env();
    let mut rendered_jobs = Vec::with_capacity(scrubbed.len());
    for (id, job) in &scrubbed {
        rendered_jobs.push((
            id.clone(),
            suite_job_to_yaml(
                id,
                job,
                &workflow_env,
                mbx_jobs.contains(id),
                allow_failure_jobs.contains(id),
            )?,
        ));
    }
    let cancel_in_progress = entries
        .iter()
        .find_map(|suite| suite.cancel_in_progress)
        .unwrap_or(false);
    let cancel_yaml =
        if cancel_in_progress && entries.iter().any(|suite| suite.has_schedule_trigger()) {
            Yaml::str("${{ github.event_name != 'schedule' }}")
        } else {
            Yaml::Bool(cancel_in_progress)
        };
    let top = vec![
        ("name".to_owned(), Yaml::str(workflow_display_name(slug))),
        (
            "on".to_owned(),
            triggers_yaml(entries, spec.workflow_dispatch.as_ref()),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("env".to_owned(), string_map_yaml(&workflow_env)),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str(format!(
                        "{slug}-${{{{ github.workflow }}}}-${{{{ github.ref }}}}-${{{{ github.event_name }}}}"
                    )),
                ),
                ("cancel-in-progress".to_owned(), cancel_yaml),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(rendered_jobs)),
    ];
    let mut document = Yaml::Map(top);
    matrix::suites::attach_suite_matrices(&mut document, &suite_matrices)?;
    let path = format!(".github/workflows/{slug}.yml");
    let text = render_checked_workflow(&path, &document, &spec.generator_version)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

fn finalize_suite_workflow_jobs(
    jobs: &mut BTreeMap<String, Job>,
    spec: &RustSuitesRenderSpec,
) -> Result<(), RenderError> {
    let default_target = crate::runs_on::target_for_runner(&spec.default_runs_on);
    for (id, job) in jobs.iter_mut() {
        let target = crate::runs_on::target_for_runner(&job.runs_on).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
        })?;
        let setup = if default_target == Some(target) {
            spec.mise.clone()
        } else {
            spec.mise.for_target(target)?
        };
        crate::cache_p08::ensure_tools_cache_v2(
            id,
            job,
            &setup,
            true,
            target,
            &spec.checkout_uses,
        )?;
        crate::cache_p08::check_no_legacy_rust_cache(id, job)?;
        crate::cache_p08::check_mbx_before_fetch(id, job)?;
    }
    crate::cache_p08::elect_cache_writers(jobs)?;
    crate::cache_steps::validate_mbx_lifecycles(jobs)?;
    crate::cache_doctor::check(jobs, crate::cache_doctor::DoctorScope::Ci)
}

fn workflow_display_name(slug: &str) -> String {
    if slug == DEFAULT_SUITE_WORKFLOW_SLUG {
        "Suites".to_owned()
    } else {
        format!("Suites ({slug})")
    }
}

fn suite_job_condition(suite: &RustSuiteConfig) -> Option<String> {
    let name = &suite.id;
    let dispatch_expr = if suite.has_workflow_dispatch_trigger() {
        let mut clauses = vec!["github.event_name == 'workflow_dispatch'".to_owned()];
        if suite.dispatch_inputs.is_empty() {
            clauses.push(format!(
                "(inputs.suite == 'all' || inputs.suite == '{name}')"
            ));
        } else {
            for (input_name, values) in &suite.dispatch_inputs {
                let value_ors: Vec<String> = values
                    .iter()
                    .map(|val| format!("inputs.{input_name} == '{val}'"))
                    .collect();
                clauses.push(if value_ors.len() == 1 {
                    value_ors[0].clone()
                } else {
                    format!("({})", value_ors.join(" || "))
                });
            }
        }
        Some(clauses.join(" && "))
    } else {
        None
    };
    let schedule_expr = if suite.has_schedule_trigger() {
        let cron_ors: Vec<String> = suite
            .schedule
            .iter()
            .map(|cron| format!("github.event.schedule == '{cron}'"))
            .collect();
        let cron_clause = match cron_ors.as_slice() {
            [] => None,
            [single] => Some(single.clone()),
            many => Some(format!("({})", many.join(" || "))),
        };
        Some(match cron_clause {
            Some(clause) => format!("github.event_name == 'schedule' && {clause}"),
            None => "github.event_name == 'schedule'".to_owned(),
        })
    } else {
        None
    };
    match (dispatch_expr, schedule_expr) {
        (Some(d), Some(s)) => Some(format!("({d}) || ({s})")),
        (Some(d), None) => Some(d),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

fn triggers_yaml(
    entries: &[&RustSuiteConfig],
    workflow_dispatch: Option<&WorkflowDispatchConfig>,
) -> Yaml {
    let dispatch_suites: Vec<&str> = entries
        .iter()
        .filter(|suite| suite.has_workflow_dispatch_trigger())
        .map(|suite| suite.id.as_str())
        .collect();
    let needs_auto_suite_input = entries
        .iter()
        .any(|suite| suite.has_workflow_dispatch_trigger() && suite.dispatch_inputs.is_empty());
    let mut crons = BTreeSet::new();
    for suite in entries {
        if suite.has_schedule_trigger() {
            for cron in &suite.schedule {
                crons.insert(cron.clone());
            }
        }
    }
    let mut triggers = Vec::new();
    if !dispatch_suites.is_empty() {
        triggers.push((
            "workflow_dispatch".to_owned(),
            Yaml::Map(vec![(
                "inputs".to_owned(),
                dispatch_inputs_yaml(&dispatch_suites, needs_auto_suite_input, workflow_dispatch),
            )]),
        ));
    }
    if !crons.is_empty() {
        let schedule_seq: Vec<Yaml> = crons
            .into_iter()
            .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron))]))
            .collect();
        triggers.push(("schedule".to_owned(), Yaml::Seq(schedule_seq)));
    }
    Yaml::Map(triggers)
}

fn dispatch_inputs_yaml(
    dispatch_suites: &[&str],
    needs_auto_suite_input: bool,
    workflow_dispatch: Option<&WorkflowDispatchConfig>,
) -> Yaml {
    let mut inputs = Vec::new();
    if needs_auto_suite_input {
        let mut options = Vec::with_capacity(dispatch_suites.len() + 1);
        options.push(Yaml::str("all"));
        for name in dispatch_suites {
            options.push(Yaml::str((*name).to_owned()));
        }
        let suite_input = Yaml::Map(vec![
            (
                "description".to_owned(),
                Yaml::str("Test suite to run ('all' runs every dispatchable suite)"),
            ),
            ("required".to_owned(), Yaml::Bool(false)),
            ("type".to_owned(), Yaml::str("choice")),
            ("default".to_owned(), Yaml::str("all")),
            ("options".to_owned(), Yaml::Seq(options)),
        ]);
        inputs.push(("suite".to_owned(), suite_input));
    }
    if let Some(dispatch) = workflow_dispatch {
        for (name, cfg) in &dispatch.inputs {
            let mut fields = Vec::new();
            if let Some(description) = &cfg.description {
                fields.push(("description".to_owned(), Yaml::str(description.clone())));
            }
            fields.push(("required".to_owned(), Yaml::Bool(cfg.required)));
            fields.push(("type".to_owned(), Yaml::str(cfg.kind.as_str().to_owned())));
            if let Some(default) = &cfg.default {
                fields.push(("default".to_owned(), Yaml::str(default.clone())));
            }
            if !cfg.options.is_empty() {
                fields.push((
                    "options".to_owned(),
                    Yaml::Seq(cfg.options.iter().cloned().map(Yaml::str).collect()),
                ));
            }
            inputs.push((name.clone(), Yaml::Map(fields)));
        }
    }
    Yaml::Map(inputs)
}

fn suite_job_to_yaml(
    id: &str,
    job: &Job,
    workflow_env: &BTreeMap<String, String>,
    native_mbx: bool,
    allow_failure: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(&runner, RunsOn::ScaleSet(_));
    let runs_on = match runner {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    };
    let job_env = extract_suite_job_env(job, native_mbx);
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if scale_set {
        entries.push(crate::runs_on::run_shell_defaults_field(
            crate::runs_on::SCALE_SET_RUN_SHELL,
        ));
    }
    let local_job_env = crate::document_env::exclude_inherited(&job_env, workflow_env);
    if !local_job_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&local_job_env)));
    }
    if let Some(permissions) = &job.permissions {
        let level = match permissions.contents {
            PermissionLevel::None => "none",
            PermissionLevel::Read => "read",
            PermissionLevel::Write => "write",
        };
        entries.push((
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str(level))]),
        ));
    }
    if !job.needs.is_empty() {
        entries.push((
            "needs".to_owned(),
            Yaml::Seq(job.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    if allow_failure {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    let mut effective_env = workflow_env.clone();
    effective_env.extend(job_env);
    let mut rendered_steps = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered_steps.push(non_internal_step_to_yaml(
            id,
            step,
            &effective_env,
            &job.runs_on,
        )?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
}

fn extract_suite_job_env(job: &Job, native_mbx: bool) -> BTreeMap<String, String> {
    let mut job_env = BTreeMap::new();
    for step in &job.steps {
        if let StepKind::Shell { env, .. } | StepKind::Action { env, .. } = &step.kind
            && let Some(toolchain) = env.get("RUSTUP_TOOLCHAIN")
        {
            job_env.insert("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone());
        }
    }
    if native_mbx {
        crate::mbx_job_env::extend_mbx_job_env(&mut job_env);
    }
    job_env
}
