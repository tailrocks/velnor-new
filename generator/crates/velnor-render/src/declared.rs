// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use velnor_model::{DeclaredBody, DeclaredJob, DeclaredStep, DeclaredWorkflow, RepoPath};

use crate::declared_expr::{Scope, bare, check_expression, check_text};
use crate::gate::all_conditions;
use crate::rules::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, RuleViolation, Value};
use crate::steps::{CACHE_RESTORE, CACHE_SAVE, CHECKOUT, DOWNLOAD_ARTIFACT, MISE, UPLOAD_ARTIFACT};
use crate::workflow::yaml_job;
use crate::yaml::{Yaml, emit, entry, text};
use crate::{GENERATED_HEADER, check_rules};

const ACTIONS: [(&str, &str); 6] = [
    ("checkout", CHECKOUT),
    ("mise", MISE),
    ("upload-artifact", UPLOAD_ARTIFACT),
    ("download-artifact", DOWNLOAD_ARTIFACT),
    ("cache-restore", CACHE_RESTORE),
    ("cache-save", CACHE_SAVE),
];
const EVENTS: [&str; 5] = [
    "workflow_call",
    "workflow_dispatch",
    "push",
    "pull_request",
    "merge_group",
];
const SCOPES: [&str; 13] = [
    "actions",
    "artifact-metadata",
    "attestations",
    "checks",
    "contents",
    "deployments",
    "id-token",
    "issues",
    "packages",
    "pages",
    "pull-requests",
    "security-events",
    "statuses",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredRule {
    BadIdentifier(String),
    UnknownAction(String),
    BadFile(String),
    EventNotAllowed(String),
    BadPermission(String),
    BadTimeout(u16),
    DuplicateJob(String),
    DuplicateStep(String),
    NeedNotDeclaredEarlier(String),
    NeedNotNamed(String),
    UndeclaredStep(String),
    ContextNotAllowed(String),
    UnclosedExpression(String),
}

impl fmt::Display for DeclaredRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadIdentifier(id) => write!(
                f,
                "{id:?} is not a valid identifier: use letters, digits, - and _"
            ),
            Self::UnknownAction(name) => write!(
                f,
                "uses {name:?} is not one of the pinned actions {:?}",
                ACTIONS.map(|(known, _)| known)
            ),
            Self::BadFile(file) => write!(
                f,
                "file {file:?} must be a plain name ending in .yml or .yaml"
            ),
            Self::EventNotAllowed(event) => write!(f, "event {event} is not one of {EVENTS:?}"),
            Self::BadPermission(pair) => {
                write!(f, "permission {pair} is not a valid scope and access")
            }
            Self::BadTimeout(minutes) => write!(f, "timeout_minutes {minutes} is outside 1..=360"),
            Self::DuplicateJob(id) => write!(f, "job {id} is declared twice"),
            Self::DuplicateStep(id) => write!(f, "step id {id} is declared twice"),
            Self::NeedNotDeclaredEarlier(id) => write!(
                f,
                "needs names {id}, which is not a job declared before this one"
            ),
            Self::NeedNotNamed(id) => write!(
                f,
                "an expression reads needs.{id}, which this job does not list in needs"
            ),
            Self::UndeclaredStep(id) => write!(
                f,
                "an expression reads steps.{id}, which is not a step id of this job"
            ),
            Self::ContextNotAllowed(reference) => write!(
                f,
                "{reference} is not an allowed context: use needs.<job>.outputs.<name>, steps.<id>.outputs.<name>, job.* or github.token"
            ),
            Self::UnclosedExpression(text) => write!(f, "{text:?} opens ${{{{ without closing it"),
        }
    }
}

fn ident(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn permission_rules(pairs: &[(String, String)]) -> Vec<DeclaredRule> {
    pairs
        .iter()
        .filter(|(scope, access)| {
            let known = SCOPES.contains(&scope.as_str());
            let level = matches!(access.as_str(), "read" | "write" | "none")
                && !(scope == "id-token" && access == "read");
            !(known && level)
        })
        .map(|(scope, access)| DeclaredRule::BadPermission(format!("{scope}: {access}")))
        .collect()
}

fn action_pin(name: &str) -> Option<&'static str> {
    ACTIONS
        .iter()
        .find_map(|(known, pin)| (*known == name).then_some(*pin))
}

pub fn render_declared(
    workflow: &DeclaredWorkflow,
    runner: &str,
    timeout_minutes: u16,
) -> Result<BTreeMap<RepoPath, String>, Vec<RuleViolation>> {
    let mut found = Vec::new();
    check_workflow(workflow, &mut found);
    if !found.is_empty() {
        return Err(found);
    }
    let rendered = RenderedWorkflow {
        jobs: workflow
            .jobs
            .iter()
            .map(|job| rendered_job(workflow, job, runner, timeout_minutes))
            .collect(),
    };
    check_rules(&rendered)?;
    let items = [
        entry("name", text(&workflow.name)),
        entry(
            "on",
            Yaml::Map(
                workflow
                    .on
                    .iter()
                    .map(|event| entry(event, Yaml::Map(Vec::new())))
                    .collect(),
            ),
        ),
        entry("permissions", pairs(&workflow.permissions)),
        entry(
            "jobs",
            Yaml::Map(
                rendered
                    .jobs
                    .iter()
                    .map(|job| (job.id.clone(), yaml_job(job)))
                    .collect(),
            ),
        ),
    ];
    let path = RepoPath::new(format!(".github/workflows/{}", workflow.file))
        .map_err(|_| violation("", DeclaredRule::BadFile(workflow.file.clone())))?;
    Ok(BTreeMap::from([(
        path,
        format!("{GENERATED_HEADER}{}", emit(&items)),
    )]))
}

fn pairs(items: &[(String, String)]) -> Yaml {
    Yaml::Map(items.iter().map(|(k, v)| entry(k, text(v))).collect())
}

fn violation(job: &str, rule: DeclaredRule) -> Vec<RuleViolation> {
    vec![RuleViolation {
        job: job.to_owned(),
        step: 0,
        rule: Rule::Declared(rule),
    }]
}

fn check_workflow(workflow: &DeclaredWorkflow, found: &mut Vec<RuleViolation>) {
    let mut add = |job: &str, rules: Vec<DeclaredRule>| {
        for rule in rules {
            found.extend(violation(job, rule));
        }
    };
    let file = &workflow.file;
    let plain = file.ends_with(".yml") || file.ends_with(".yaml");
    if !plain
        || file.starts_with('.')
        || !file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        add("", vec![DeclaredRule::BadFile(file.clone())]);
    }
    add(
        "",
        workflow
            .on
            .iter()
            .filter(|event| !EVENTS.contains(&event.as_str()))
            .map(|event| DeclaredRule::EventNotAllowed(event.clone()))
            .chain(permission_rules(&workflow.permissions))
            .collect(),
    );
    let mut earlier: Vec<String> = Vec::new();
    for job in &workflow.jobs {
        add(&job.id, check_job(workflow, job, &earlier));
        earlier.push(job.id.clone());
    }
}

fn check_job(
    workflow: &DeclaredWorkflow,
    job: &DeclaredJob,
    earlier: &[String],
) -> Vec<DeclaredRule> {
    let mut rules = Vec::new();
    if !ident(&job.id) {
        rules.push(DeclaredRule::BadIdentifier(job.id.clone()));
    }
    if earlier.contains(&job.id) {
        rules.push(DeclaredRule::DuplicateJob(job.id.clone()));
    }
    rules.extend(
        job.needs
            .iter()
            .filter(|need| !earlier.contains(need))
            .map(|need| DeclaredRule::NeedNotDeclaredEarlier(need.clone())),
    );
    if job
        .timeout_minutes
        .is_some_and(|minutes| !(1..=360).contains(&minutes))
    {
        rules.push(DeclaredRule::BadTimeout(job.timeout_minutes.unwrap_or(0)));
    }
    rules.extend(permission_rules(&job.permissions));
    let ids: Vec<String> = job
        .steps
        .iter()
        .filter_map(|step| step.id.clone())
        .collect();
    for (index, id) in ids.iter().enumerate() {
        if !ident(id) {
            rules.push(DeclaredRule::BadIdentifier(id.clone()));
        }
        if ids[..index].contains(id) {
            rules.push(DeclaredRule::DuplicateStep(id.clone()));
        }
    }
    let job_scope = Scope {
        needs: &job.needs,
        steps: &[],
        condition: true,
    };
    let value_scope = Scope {
        needs: &job.needs,
        steps: &ids,
        condition: false,
    };
    let step_scope = Scope {
        needs: &job.needs,
        steps: &ids,
        condition: true,
    };
    for condition in workflow.condition.iter().chain(&job.condition) {
        rules.extend(check_expression(bare(condition), &job_scope));
    }
    for (name, value) in &job.outputs {
        if !ident(name) {
            rules.push(DeclaredRule::BadIdentifier(name.clone()));
        }
        rules.extend(check_expression(bare(value), &value_scope));
    }
    for step in &job.steps {
        rules.extend(check_step(step, &step_scope, &value_scope));
    }
    rules
}

fn check_step(
    step: &DeclaredStep,
    step_scope: &Scope<'_>,
    value_scope: &Scope<'_>,
) -> Vec<DeclaredRule> {
    let mut rules = Vec::new();
    match &step.body {
        DeclaredBody::Action(name) if action_pin(name).is_none() => {
            rules.push(DeclaredRule::UnknownAction(name.clone()));
        }
        DeclaredBody::Helper(command) if !ident(command) => {
            rules.push(DeclaredRule::BadIdentifier(command.clone()));
        }
        _ => {}
    }
    if let Some(condition) = &step.condition {
        rules.extend(check_expression(bare(condition), step_scope));
    }
    for (key, value) in step.with.iter().chain(&step.env) {
        if !ident(key) {
            rules.push(DeclaredRule::BadIdentifier(key.clone()));
        }
        rules.extend(check_text(value, value_scope));
    }
    rules
}

fn rendered_job(
    workflow: &DeclaredWorkflow,
    job: &DeclaredJob,
    runner: &str,
    timeout_minutes: u16,
) -> RenderedJob {
    let wrap = |condition: &Option<String>| {
        condition
            .as_deref()
            .map(|condition| format!("${{{{ {} }}}}", bare(condition)))
    };
    RenderedJob {
        id: job.id.clone(),
        needs: job.needs.clone(),
        condition: all_conditions(wrap(&workflow.condition), wrap(&job.condition)),
        environment: job.environment.clone(),
        permissions: job.permissions.clone(),
        outputs: job
            .outputs
            .iter()
            .map(|(name, value)| (name.clone(), format!("${{{{ {} }}}}", bare(value))))
            .collect(),
        runs_on: runner.to_owned(),
        timeout_minutes: job.timeout_minutes.unwrap_or(timeout_minutes),
        steps: job.steps.iter().map(rendered_step).collect(),
        ..RenderedJob::default()
    }
}

fn rendered_step(step: &DeclaredStep) -> RenderedStep {
    let value = |text: &String| {
        if text.contains("${{") {
            Value::Expr(text.clone())
        } else {
            Value::Text(text.clone())
        }
    };
    let (label, uses, run) = match &step.body {
        DeclaredBody::Action(name) => (name.clone(), action_pin(name).map(str::to_owned), None),
        DeclaredBody::Helper(command) => {
            (command.clone(), None, Some(format!("velnor-ci {command}")))
        }
    };
    RenderedStep {
        id: step.id.clone(),
        name: step.name.clone().unwrap_or(label),
        condition: step
            .condition
            .as_deref()
            .map(|condition| format!("${{{{ {} }}}}", bare(condition))),
        uses,
        with: step
            .with
            .iter()
            .map(|(k, v)| (k.clone(), value(v)))
            .collect(),
        env: step
            .env
            .iter()
            .map(|(k, v)| (k.clone(), value(v)))
            .collect(),
        run,
        ..RenderedStep::default()
    }
}
