use std::collections::BTreeMap;

use crate::RenderError;
use crate::document_steps::string_map_yaml;
use crate::steps;
use crate::yaml::Yaml;
use velnor_actions_contract::Job;

use super::permissions_to_yaml;

pub(super) fn job_header_fields(job: &Job, runs_on: Yaml) -> Vec<(String, Yaml)> {
    let timeout = Yaml::Int(i64::from(job.timeout_minutes.minutes()));
    vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), timeout),
    ]
}

pub(super) fn append_job_options(
    entries: &mut Vec<(String, Yaml)>,
    job: &Job,
    scale_set: bool,
    job_env: &BTreeMap<String, String>,
    workflow_env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if scale_set {
        entries.push(crate::runs_on::run_shell_defaults_field(
            crate::runs_on::SCALE_SET_RUN_SHELL,
        ));
    }
    let job_env = crate::document_env::exclude_inherited(job_env, workflow_env);
    if !job_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&job_env)));
    }
    if let Some(environment) = &job.environment {
        entries.push(("environment".to_owned(), Yaml::str(environment.clone())));
    }
    if let Some(permissions) = &job.permissions {
        entries.push(("permissions".to_owned(), permissions_to_yaml(permissions)));
    }
    if let Some(cache_mode) = &job.cache_mode {
        steps::scan_for_private_subcommands(cache_mode.as_str())?;
        entries.push(Yaml::cache_mode_field(cache_mode));
    }
    if !job.needs.is_empty() {
        let needs: Vec<Yaml> = job
            .needs
            .iter()
            .map(|need| Yaml::str(need.clone()))
            .collect();
        entries.push(("needs".to_owned(), Yaml::Seq(needs)));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    Ok(())
}
