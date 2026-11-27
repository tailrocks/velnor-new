// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use super::adapter::{AdapterCall, invoke};
use super::contexts::set_output;
use super::env_files::{Collected, FileCommands, failure, save_summary};
use super::evaluate;
use super::job::Job;
use super::output::Output;
use super::preflight::reference;
use super::shell;
use super::step::{Limits, ScriptStep, run_script};
use super::step_info::step_timeout;
use super::timeout::{Survival, Timeout, settle, stop_unless_surviving};
use super::{AnnotationLevel, ExecError, Inputs, JobCancel, StepResult};
use crate::expr::{ExprEnv, JobStatus};
use crate::job_message::ActionStep;

pub(super) struct Executed {
    pub(super) result: StepResult,
    pub(super) outputs: Vec<(String, String)>,
    pub(super) summary: Option<PathBuf>,
    pub(super) env: Vec<(String, String)>,
}

type Ran = Result<(StepResult, Vec<(String, String)>), ExecError>;

impl Job<'_> {
    pub(super) async fn execute_watched(
        &mut self,
        step: &ActionStep,
        step_env: Vec<(String, String)>,
        env: &ExprEnv,
        step_vars: &[(String, String)],
        already_cancelled: bool,
        out: &mut Output<'_>,
    ) -> Result<Executed, ExecError> {
        let halt = JobCancel::new();
        let survival = if already_cancelled {
            None
        } else {
            Some(Survival::new(
                step.condition.as_deref().unwrap_or_default(),
                self.ctx.expr_env(JobStatus::Cancelled, step_vars)?,
            ))
        };
        let watcher = stop_unless_surviving(self.cancel.clone(), halt.clone(), survival.as_ref());
        let signal = survival.is_some().then_some(&halt);
        tokio::select! {
            executed = self.execute(step, step_env, env, (signal, survival.as_ref()), out) => executed,
            never = watcher => match never {},
        }
    }

    async fn execute(
        &mut self,
        step: &ActionStep,
        step_env: Vec<(String, String)>,
        env: &ExprEnv,
        (cancel, survival): (Option<&JobCancel>, Option<&Survival<'_>>),
        out: &mut Output<'_>,
    ) -> Result<Executed, ExecError> {
        let timeout = Timeout::start(step_timeout(step, env, out).await);
        let files = FileCommands::create(&self.options.temp).await?;
        let process_env = self.ctx.process_env(&step_env, &files.variables());
        let limits = Limits {
            timeout,
            cancel,
            groups: &self.groups,
        };
        let ran = if let Some(reference) = reference(step) {
            self.run_adapter(step, &reference, env, &process_env, &limits, out)
                .await
        } else {
            self.run_script_step(step, env, process_env, &limits, out)
                .await
        };
        let (result, adapter_outputs) = match ran {
            Ok(done) => done,
            Err(error) => {
                out.message(&error.to_string());
                out.annotate(AnnotationLevel::Error, error.to_string());
                (StepResult::Failed, Vec::new())
            }
        };
        let result = settle(result, survival, &self.cancel, timeout, out).await;
        let collected = files.collect().await;
        self.finish(result, adapter_outputs, collected, out).await
    }

    async fn run_script_step(
        &self,
        step: &ActionStep,
        env: &ExprEnv,
        process_env: Vec<(String, String)>,
        limits: &Limits<'_>,
        out: &mut Output<'_>,
    ) -> Ran {
        let script = ScriptStep {
            script: evaluate::input(step.inputs.as_ref(), "script", env)
                .await?
                .unwrap_or_default(),
            shell: shell::resolve(step, &self.ctx, env).await?,
            dir: self.working_directory(step, env).await?,
            env: process_env,
        };
        let result = run_script(&script, &self.options.temp, limits, out).await?;
        Ok((result, Vec::new()))
    }

    async fn working_directory(
        &self,
        step: &ActionStep,
        env: &ExprEnv,
    ) -> Result<PathBuf, ExecError> {
        let chosen = match evaluate::input(step.inputs.as_ref(), "workingDirectory", env).await? {
            Some(directory) => Some(directory),
            None => evaluate::input(step.inputs.as_ref(), "working-directory", env).await?,
        };
        let directory = match chosen.filter(|directory| !directory.is_empty()) {
            Some(directory) => Some(directory),
            None => self.ctx.default_run("working-directory").map(str::to_owned),
        };
        Ok(directory.map_or_else(
            || self.options.workspace.clone(),
            |directory| self.options.workspace.join(directory),
        ))
    }

    async fn run_adapter(
        &self,
        step: &ActionStep,
        reference: &str,
        env: &ExprEnv,
        process_env: &[(String, String)],
        limits: &Limits<'_>,
        out: &mut Output<'_>,
    ) -> Ran {
        let adapter = self
            .adapters
            .find(reference)
            .ok_or_else(|| ExecError::Unsupported(format!("uses {reference}")))?;
        let inputs = match step.inputs.as_ref() {
            Some(inputs) => evaluate::mapping(inputs, env).await?.into_iter().collect(),
            None => Inputs::default(),
        };
        let call = AdapterCall {
            reference: reference.to_owned(),
            inputs,
            env: process_env.iter().cloned().collect(),
            workspace: &self.options.workspace,
            temp: &self.options.temp,
            cancel: limits.cancel.cloned(),
            timeout: limits.timeout,
            groups: limits.groups,
        };
        invoke(adapter, call, out).await
    }

    async fn finish(
        &mut self,
        result: StepResult,
        adapter_outputs: Vec<(String, String)>,
        collected: Collected,
        out: &mut Output<'_>,
    ) -> Result<Executed, ExecError> {
        let mut result = result;
        let command_failed = out.issues.take_command_failure();
        let mut failures = collected.failures;
        let summary = match save_summary(&self.options.temp, &collected.summary, out).await {
            Ok(summary) => summary,
            Err(error) => {
                failures.extend(failure("step_summary", &error));
                None
            }
        };
        if (command_failed || !failures.is_empty()) && result == StepResult::Succeeded {
            result = StepResult::Failed;
        }
        for failure in &failures {
            out.message(failure);
            out.annotate(AnnotationLevel::Error, failure.clone());
        }
        let mut writes = Vec::new();
        for (name, value) in collected.env {
            if name.eq_ignore_ascii_case("NODE_OPTIONS") {
                out.annotate(
                    AnnotationLevel::Error,
                    "Can't store NODE_OPTIONS output parameter using '$GITHUB_ENV' command.",
                );
            } else {
                writes.push((name, value));
            }
        }
        self.ctx.apply_env(&writes);
        self.ctx.prepend_path(&collected.path);
        let mut outputs = adapter_outputs;
        for (name, value) in &collected.outputs {
            set_output(&mut outputs, name, value);
        }
        Ok(Executed {
            result,
            outputs,
            summary,
            env: writes,
        })
    }
}
