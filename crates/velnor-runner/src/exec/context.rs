// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde_json::Value as Json;

use super::token;
use super::{ExecError, RunOptions, StepResult};
use crate::context_value::ContextValue;
use crate::expr::{ExprEnv, JobStatus, RootContext};
use crate::job_message::AgentJobRequestMessage;

const GITHUB_EXPORTS: [&str; 37] = [
    "action",
    "action_path",
    "action_ref",
    "action_repository",
    "actor",
    "actor_id",
    "api_url",
    "base_ref",
    "env",
    "event_name",
    "event_path",
    "graphql_url",
    "head_ref",
    "job",
    "output",
    "path",
    "ref_name",
    "ref_protected",
    "ref_type",
    "ref",
    "repository",
    "repository_id",
    "repository_owner",
    "repository_owner_id",
    "retention_days",
    "run_attempt",
    "run_id",
    "run_number",
    "server_url",
    "sha",
    "state",
    "step_summary",
    "triggering_actor",
    "workflow",
    "workflow_ref",
    "workflow_sha",
    "workspace",
];

const SELF_HOSTED: &str = "self-hosted";
const EVENT_DIRECTORY: &str = "_github_workflow";
const GITHUB_TOKEN_VARIABLE: &str = "system.github.token";
const ACCESS_TOKEN_VARIABLE: &str = "system.accesstoken";

#[derive(Debug, Clone)]
pub(super) struct StepEntry {
    pub(super) outcome: StepResult,
    pub(super) conclusion: StepResult,
    pub(super) outputs: Vec<(String, String)>,
}

#[derive(Debug)]
pub(super) struct JobContext {
    base: ExprEnv,
    runtime: Vec<(String, String)>,
    env: Vec<(String, String)>,
    defaults: Vec<Json>,
    path_prepend: Vec<String>,
    steps: Vec<(String, StepEntry)>,
}

pub(super) fn set_pair(pairs: &mut Vec<(String, String)>, name: &str, value: &str) {
    pairs.retain(|(known, _)| !known.eq_ignore_ascii_case(name));
    pairs.push((name.to_owned(), value.to_owned()));
}

fn string(value: &str) -> ContextValue {
    ContextValue::String(value.to_owned())
}

fn object(pairs: &[(String, String)]) -> ContextValue {
    ContextValue::Object {
        case_sensitive: false,
        entries: pairs
            .iter()
            .map(|(name, value)| (name.clone(), string(value)))
            .collect(),
    }
}

fn variable<'a>(message: &'a AgentJobRequestMessage, name: &str) -> Option<&'a str> {
    message
        .variables
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .and_then(|(_, variable)| variable.value.as_ref().map(|value| value.expose().as_str()))
}

fn upsert(entries: &mut Vec<(String, ContextValue)>, name: &str, value: ContextValue) {
    entries.retain(|(known, _)| !known.eq_ignore_ascii_case(name));
    entries.push((name.to_owned(), value));
}

fn secrets_context(message: &AgentJobRequestMessage) -> ContextValue {
    let pairs: Vec<(String, String)> = message
        .variables
        .iter()
        .filter(|(name, variable)| {
            variable.is_secret
                && !name.eq_ignore_ascii_case(ACCESS_TOKEN_VARIABLE)
                && !name.eq_ignore_ascii_case(GITHUB_TOKEN_VARIABLE)
        })
        .filter_map(|(name, variable)| {
            Some((name.clone(), variable.value.as_ref()?.expose().clone()))
        })
        .collect();
    object(&pairs)
}

fn github_context(
    message: &AgentJobRequestMessage,
    source: Option<&ContextValue>,
    options: &RunOptions,
    event_path: &Path,
) -> ContextValue {
    let mut entries = source
        .and_then(ContextValue::entries)
        .map_or_default(<[_]>::to_vec);
    if let Some(value) = variable(message, GITHUB_TOKEN_VARIABLE) {
        upsert(&mut entries, "token", string(value));
    }
    if let Some(value) = variable(message, "system.github.job") {
        upsert(&mut entries, "job", string(value));
    }
    upsert(
        &mut entries,
        "workspace",
        string(&options.workspace.to_string_lossy()),
    );
    upsert(
        &mut entries,
        "event_path",
        string(&event_path.to_string_lossy()),
    );
    ContextValue::Object {
        case_sensitive: false,
        entries,
    }
}

fn runner_context(message: &AgentJobRequestMessage, options: &RunOptions) -> ContextValue {
    let arch = if cfg!(target_arch = "aarch64") {
        "ARM64"
    } else {
        "X64"
    };
    let environment = variable(message, "system.runnerEnvironment").unwrap_or(SELF_HOSTED);
    object(&[
        ("os".to_owned(), "Linux".to_owned()),
        ("arch".to_owned(), arch.to_owned()),
        ("name".to_owned(), options.runner_name.clone()),
        (
            "temp".to_owned(),
            options.temp.to_string_lossy().into_owned(),
        ),
        (
            "tool_cache".to_owned(),
            options
                .temp
                .join("tool_cache")
                .to_string_lossy()
                .into_owned(),
        ),
        ("environment".to_owned(), environment.to_owned()),
    ])
}

fn exported_variables(github: &ContextValue, runner: &ContextValue) -> Vec<(String, String)> {
    let mut variables = vec![("GITHUB_ACTIONS".to_owned(), "true".to_owned())];
    let github_entries = github.entries().unwrap_or_default();
    let allowed = github_entries.iter().filter(|(name, _)| {
        GITHUB_EXPORTS
            .iter()
            .any(|key| key.eq_ignore_ascii_case(name))
    });
    let runner_entries = runner.entries().unwrap_or_default();
    let prefixed = allowed
        .map(|entry| ("GITHUB_", entry))
        .chain(runner_entries.iter().map(|entry| ("RUNNER_", entry)));
    for (prefix, (name, value)) in prefixed {
        let text = match value {
            ContextValue::String(text) => text.clone(),
            ContextValue::Bool(flag) => flag.to_string(),
            _ => continue,
        };
        set_pair(
            &mut variables,
            &format!("{prefix}{}", name.to_ascii_uppercase()),
            &text,
        );
    }
    variables
}

async fn write_event(path: &Path, github: &ContextValue) -> Result<(), ExecError> {
    let body = github
        .get("event")
        .map(ContextValue::to_github_json)
        .transpose()
        .map_err(|error| ExecError::Invalid(format!("github.event: {error}")))?
        .unwrap_or_else(|| "{}".to_owned());
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| ExecError::io(format!("create {}", parent.display()), source))?;
    }
    tokio::fs::write(path, body)
        .await
        .map_err(|source| ExecError::io(format!("write {}", path.display()), source))
}

impl JobContext {
    pub(super) async fn new(
        message: &AgentJobRequestMessage,
        options: &RunOptions,
    ) -> Result<Self, ExecError> {
        for directory in [&options.workspace, &options.temp] {
            tokio::fs::create_dir_all(directory)
                .await
                .map_err(|source| {
                    ExecError::io(format!("create {}", directory.display()), source)
                })?;
        }
        let contexts = message
            .materialize_context_values()
            .map_err(|error| ExecError::Invalid(format!("{error:#}")))?
            .into_inner();
        let event_path = options.temp.join(EVENT_DIRECTORY).join("event.json");
        let github = github_context(message, contexts.get("github"), options, &event_path);
        write_event(&event_path, &github).await?;
        let runner = runner_context(message, options);
        let mut base = ExprEnv::new(options.workspace.clone());
        base.set_context(RootContext::Github, &github);
        base.set_context(RootContext::Runner, &runner);
        base.set_context(RootContext::Secrets, &secrets_context(message));
        for (name, root) in [
            ("needs", RootContext::Needs),
            ("inputs", RootContext::Inputs),
        ] {
            if let Some(value) = contexts.get(name) {
                base.set_context(root, value);
            }
        }
        let mut env = Vec::new();
        for token in &message.environment_variables {
            for (name, value) in token::mapping(token, &base)? {
                set_pair(&mut env, &name, &value);
            }
        }
        Ok(Self {
            runtime: exported_variables(&github, &runner),
            base,
            env,
            defaults: message.defaults.clone(),
            path_prepend: Vec::new(),
            steps: Vec::new(),
        })
    }

    pub(super) fn base_env(&self) -> &ExprEnv {
        &self.base
    }

    pub(super) fn expr_env(&self, status: JobStatus, step_env: &[(String, String)]) -> ExprEnv {
        let mut env = self.base.clone();
        env.status = status;
        let mut merged = self.env.clone();
        for (name, value) in step_env {
            set_pair(&mut merged, name, value);
        }
        env.set_context(RootContext::Env, &object(&merged));
        env.set_context(RootContext::Steps, &self.steps_value());
        env
    }

    fn steps_value(&self) -> ContextValue {
        ContextValue::Object {
            case_sensitive: false,
            entries: self
                .steps
                .iter()
                .map(|(id, entry)| {
                    let fields = vec![
                        ("outputs".to_owned(), object(&entry.outputs)),
                        ("outcome".to_owned(), string(entry.outcome.status_name())),
                        (
                            "conclusion".to_owned(),
                            string(entry.conclusion.status_name()),
                        ),
                    ];
                    (
                        id.clone(),
                        ContextValue::Object {
                            case_sensitive: false,
                            entries: fields,
                        },
                    )
                })
                .collect(),
        }
    }

    pub(super) fn default_run_value(
        &self,
        name: &str,
        env: &ExprEnv,
    ) -> Result<Option<String>, ExecError> {
        let mut found = None;
        for defaults in &self.defaults {
            if let Some(value) = token::find(defaults, "run").and_then(|run| token::find(run, name))
            {
                found = Some(token::scalar(value, env)?);
            }
        }
        Ok(found)
    }

    pub(super) fn process_env(
        &self,
        step_env: &[(String, String)],
        file_variables: &[(&'static str, String)],
    ) -> Vec<(String, String)> {
        let mut env = self.env.clone();
        for (name, value) in step_env.iter().chain(&self.runtime) {
            set_pair(&mut env, name, value);
        }
        for (name, value) in file_variables {
            set_pair(&mut env, name, value);
        }
        if std::env::var_os("CI").is_none() && !env.iter().any(|(name, _)| name == "CI") {
            set_pair(&mut env, "CI", "true");
        }
        if !self.path_prepend.is_empty() {
            let base = env
                .iter()
                .find(|(name, _)| name == "PATH")
                .map(|(_, value)| value.clone())
                .or_else(|| std::env::var("PATH").ok());
            let mut entries = self.path_prepend.clone();
            entries.extend(base);
            set_pair(&mut env, "PATH", &entries.join(":"));
        }
        env
    }

    pub(super) fn record_step(&mut self, id: &str, entry: StepEntry) {
        self.steps
            .retain(|(known, _)| !known.eq_ignore_ascii_case(id));
        self.steps.push((id.to_owned(), entry));
    }

    pub(super) fn apply_env(&mut self, writes: &[(String, String)]) {
        for (name, value) in writes {
            set_pair(&mut self.env, name, value);
        }
    }

    pub(super) fn prepend_path(&mut self, entries: &[String]) {
        for entry in entries {
            self.path_prepend.retain(|known| known != entry);
            self.path_prepend.insert(0, entry.clone());
        }
    }
}
