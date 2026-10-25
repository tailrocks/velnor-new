//! Build workflow triggers, display names, and dispatch inputs for suite documents.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    DEFAULT_SUITE_WORKFLOW_SLUG, RustSuiteConfig, WorkflowDispatchConfig,
};

use crate::yaml::Yaml;

pub(super) fn workflow_display_name(slug: &str) -> String {
    if slug == DEFAULT_SUITE_WORKFLOW_SLUG {
        "Suites".to_owned()
    } else {
        format!("Suites ({slug})")
    }
}

pub(super) fn suite_job_condition(suite: &RustSuiteConfig) -> Option<String> {
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

pub(super) fn triggers_yaml(
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
