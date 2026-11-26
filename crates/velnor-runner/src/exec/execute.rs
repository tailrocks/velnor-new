// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::adapter::{AdapterCall, invoke};
use super::contexts::set_output;
use super::env_files::{Collected, FileCommands, save_summary};
use super::job::Job;
use super::output::Output;
use super::preflight::reference;
use super::shell;
use super::step::{Limits, ScriptStep, run_script};
use super::token;
use super::{AnnotationLevel, ExecError, JobCancel, StepResult};
use crate::expr::ExprEnv;
use crate::job_message::ActionStep;

pub(super) struct Executed {
    pub(super) result: StepResult,
    pub(super) outputs: Vec<(String, String)>,
    pub(super) summary: Option<PathBuf>,
}

type Ran = Result<(StepResult, Vec<(String, String)>), ExecError>;

impl Job<'_> {
    pub(super) async fn execute(
        &mut self,
        step: &ActionStep,
        step_env: Vec<(String, String)>,
        env: &ExprEnv,
        cancel: Option<&JobCancel>,
        out: &mut Output<'_>,
    ) -> Result<Executed, ExecError> {
        let minutes = step
            .timeout_in_minutes
            .as_ref()
            .map(|token| token::scalar(token, env))
            .transpose()?
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|minutes| minutes.is_finite() && *minutes > 0.0);
        let files = FileCommands::create(&self.options.temp).await?;
        let process_env = self.ctx.process_env(&step_env, &files.variables());
        let limits = Limits {
            minutes,
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
        let collected = files.collect().await;
        let (result, adapter_outputs) = ran?;
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
            script: token::input(step.inputs.as_ref(), "script", env)?.unwrap_or_default(),
            shell: shell::resolve(step, &self.ctx, env)?,
            dir: self.working_directory(step, env)?,
            env: process_env,
        };
        let result = run_script(&script, &self.options.temp, limits, out).await?;
        Ok((result, Vec::new()))
    }

    fn working_directory(&self, step: &ActionStep, env: &ExprEnv) -> Result<PathBuf, ExecError> {
        let chosen = match token::input(step.inputs.as_ref(), "workingDirectory", env)? {
            Some(directory) => Some(directory),
            None => token::input(step.inputs.as_ref(), "working-directory", env)?,
        };
        let directory = match chosen.filter(|directory| !directory.is_empty()) {
            Some(directory) => Some(directory),
            None => self
                .ctx
                .default_run_value("working-directory", env)?
                .filter(|directory| !directory.is_empty()),
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
            Some(inputs) => token::mapping(inputs, env)?.into_iter().collect(),
            None => BTreeMap::new(),
        };
        let call = AdapterCall {
            reference: reference.to_owned(),
            inputs,
            env: process_env.iter().cloned().collect(),
            workspace: &self.options.workspace,
            temp: &self.options.temp,
            cancel: limits.cancel.cloned(),
            minutes: limits.minutes,
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
        if (command_failed || !collected.failures.is_empty()) && result == StepResult::Succeeded {
            result = StepResult::Failed;
        }
        for failure in &collected.failures {
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
        let summary = save_summary(&self.options.temp, &collected.summary, out).await?;
        Ok(Executed {
            result,
            outputs,
            summary,
        })
    }
}
