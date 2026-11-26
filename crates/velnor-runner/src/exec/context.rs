// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use super::contexts::{
    EVENT_DIRECTORY, exported_variables, github_context, object, run_defaults, runner_context,
    secrets_context, service_variables, set_pair, string, with_action, write_event,
};
use super::host_env::host_env;
use super::{ExecError, RunOptions, StepResult};
use super::{private, token};
use crate::context_value::ContextValue;
use crate::expr::{ExprEnv, JobStatus, RootContext};
use crate::job_message::{ActionReferenceType, ActionStep, AgentJobRequestMessage};

#[derive(Debug, Clone)]
pub(super) struct StepEntry {
    pub(super) outcome: StepResult,
    pub(super) conclusion: StepResult,
    pub(super) outputs: Vec<(String, String)>,
}

#[derive(Debug)]
pub(super) struct JobContext {
    base: ExprEnv,
    github: ContextValue,
    runner: ContextValue,
    event_path: PathBuf,
    host: Vec<(String, String)>,
    runtime: Vec<(String, String)>,
    service: Vec<(String, String)>,
    env: Vec<(String, String)>,
    defaults: Vec<(String, String)>,
    path_prepend: Vec<String>,
    steps: Vec<(String, StepEntry)>,
}

impl JobContext {
    pub(super) async fn new(
        message: &AgentJobRequestMessage,
        options: &RunOptions,
    ) -> Result<Self, ExecError> {
        for (name, path) in [("workspace", &options.workspace), ("temp", &options.temp)] {
            if path.to_str().is_none() {
                return Err(ExecError::Invalid(format!(
                    "the {name} path is not valid UTF-8: {}",
                    path.display()
                )));
            }
        }
        private::create_dir(&options.temp).await?;
        private::create_dir(&options.workspace).await?;
        let contexts = message
            .materialize_context_values()
            .map_err(|error| ExecError::Invalid(format!("{error:#}")))?
            .into_inner();
        let event_path = options.temp.join(EVENT_DIRECTORY).join("event.json");
        let github = github_context(message, contexts.get("github"), options, &event_path);
        let runner = runner_context(message, options)?;
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
            base.set_context(RootContext::Env, &object(&env, true));
        }
        let defaults = run_defaults(message, &base)?;
        Ok(Self {
            host: host_env(),
            runtime: exported_variables(&github, &runner),
            service: service_variables(message),
            github,
            runner,
            event_path,
            base,
            env,
            defaults,
            path_prepend: Vec::new(),
            steps: Vec::new(),
        })
    }

    pub(super) async fn begin_step(&mut self, step: &ActionStep) -> Result<(), ExecError> {
        let repository = step
            .reference
            .as_ref()
            .filter(|reference| {
                reference.kind == ActionReferenceType::Repository
                    && !reference
                        .repository_type
                        .as_deref()
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("self"))
            })
            .and_then(|reference| {
                Some((
                    reference.name.as_deref()?,
                    reference.git_ref.as_deref().unwrap_or_default(),
                ))
            });
        self.github = with_action(
            &self.github,
            step.name.as_deref().unwrap_or_default(),
            repository,
        );
        self.base.set_context(RootContext::Github, &self.github);
        self.runtime = exported_variables(&self.github, &self.runner);
        write_event(&self.event_path, &self.github).await
    }

    pub(super) fn expr_env(&self, status: JobStatus, step_env: &[(String, String)]) -> ExprEnv {
        let mut env = self.base.clone();
        env.status = status;
        let mut merged = self.env.clone();
        for (name, value) in step_env {
            set_pair(&mut merged, name, value);
        }
        env.set_context(RootContext::Env, &object(&merged, true));
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
                        ("outputs".to_owned(), object(&entry.outputs, false)),
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

    pub(super) fn default_run(&self, name: &str) -> Option<&str> {
        self.defaults
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub(super) fn process_env(
        &self,
        step_env: &[(String, String)],
        file_variables: &[(&'static str, String)],
    ) -> Vec<(String, String)> {
        let mut env = self.host.clone();
        for (name, value) in self
            .env
            .iter()
            .chain(step_env)
            .chain(&self.runtime)
            .chain(&self.service)
        {
            set_pair(&mut env, name, value);
        }
        for (name, value) in file_variables {
            set_pair(&mut env, name, value);
        }
        if !env.iter().any(|(name, _)| name == "CI") {
            set_pair(&mut env, "CI", "true");
        }
        if !self.path_prepend.is_empty() {
            let base = env
                .iter()
                .find(|(name, _)| name == "PATH")
                .map(|(_, value)| value.clone());
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
