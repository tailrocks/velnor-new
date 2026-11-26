// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::adapter::{AdapterRegistry, StepContext};
use super::context::{JobContext, StepEntry, set_pair};
use super::env_files::{Collected, FileCommands, save_summary};
use super::mask;
use super::preflight::{problems, reference};
use super::shell;
use super::sink::LogSink;
use super::step::{Output, ScriptStep, run_script};
use super::token;
use super::{
    Annotation, AnnotationLevel, ExecError, JobCancel, JobResult, JobRun, RunOptions, StepRecord,
    StepResult,
};
use crate::expr::{ExprEnv, JobStatus, evaluate_condition, evaluate_template};
use crate::job_message::{ActionStep, AgentJobRequestMessage};

struct Job<'a> {
    message: &'a AgentJobRequestMessage,
    options: &'a RunOptions,
    adapters: &'a AdapterRegistry,
    cancel: JobCancel,
    ctx: JobContext,
    status: JobStatus,
}

struct Executed {
    result: StepResult,
    outputs: Vec<(String, String)>,
    summary: Option<std::path::PathBuf>,
}

fn context_id(step: &ActionStep) -> Option<&str> {
    step.context_name
        .as_deref()
        .filter(|name| !name.is_empty() && !name.starts_with("__"))
}

fn step_name(step: &ActionStep, env: &ExprEnv) -> String {
    step.display_name_template()
        .and_then(|template| evaluate_template(&template, env).ok())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            step.script_text()
                .and_then(|script| script.lines().next())
                .map(|line| format!("Run {line}"))
        })
        .or_else(|| reference(step))
        .or_else(|| step.name.clone())
        .unwrap_or_else(|| "step".to_owned())
}

fn continue_on_error(step: &ActionStep, env: &ExprEnv, out: &mut Output<'_>) -> bool {
    let Some(token) = step.continue_on_error.as_ref() else {
        return false;
    };
    match token::scalar(token, env) {
        Ok(value) => value.eq_ignore_ascii_case("true"),
        Err(error) => {
            out.message(&error.to_string());
            false
        }
    }
}

fn reject(out: &mut Output<'_>, run: &mut JobRun, lines: &[String]) {
    for line in lines {
        out.message(line);
        run.annotations
            .push(Annotation::new(AnnotationLevel::Error, line.clone()));
    }
}

pub async fn run_job(
    message: &AgentJobRequestMessage,
    options: &RunOptions,
    adapters: &AdapterRegistry,
    sink: &mut dyn LogSink,
    cancel: JobCancel,
) -> JobRun {
    let mut run = JobRun {
        result: JobResult::Failed,
        steps: Vec::new(),
        outputs: BTreeMap::new(),
        annotations: Vec::new(),
    };
    let mut masker = mask::for_job(message);
    let mut annotations = Vec::new();
    let mut out = Output {
        number: 0,
        sink,
        masker: &mut masker,
        annotations: &mut annotations,
    };
    match JobContext::new(message, options).await {
        Err(error) => reject(&mut out, &mut run, &[error.to_string()]),
        Ok(ctx) => {
            let unsupported: Vec<String> = problems(message, adapters, &ctx)
                .into_iter()
                .map(|problem| format!("unsupported: {problem}"))
                .collect();
            if unsupported.is_empty() {
                let mut job = Job {
                    message,
                    options,
                    adapters,
                    cancel,
                    ctx,
                    status: JobStatus::Success,
                };
                job.execute_all(&mut out, &mut run).await;
            } else {
                reject(&mut out, &mut run, &unsupported);
            }
        }
    }
    run.annotations.append(&mut annotations);
    sink.job_finished(&run);
    run
}

impl Job<'_> {
    async fn execute_all(&mut self, out: &mut Output<'_>, run: &mut JobRun) {
        for (index, step) in self.message.steps.iter().enumerate() {
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let record = self.run_step(number, step, out).await;
            run.steps.push(record);
        }
        self.collect_outputs(out, run);
        run.result = match self.status {
            JobStatus::Success => JobResult::Succeeded,
            JobStatus::Failure => JobResult::Failed,
            JobStatus::Cancelled => JobResult::Cancelled,
        };
    }

    fn collect_outputs(&mut self, out: &mut Output<'_>, run: &mut JobRun) {
        let Some(token) = self.message.job_outputs.as_ref() else {
            return;
        };
        let env = self.ctx.expr_env(self.status, &[]);
        match token::mapping(token, &env) {
            Ok(outputs) => {
                run.outputs = outputs
                    .into_iter()
                    .filter(|(_, value)| !out.masker.reveals(value))
                    .collect();
            }
            Err(error) => {
                out.message(&error.to_string());
                if self.status == JobStatus::Success {
                    self.status = JobStatus::Failure;
                }
            }
        }
    }

    async fn run_step(
        &mut self,
        number: u32,
        step: &ActionStep,
        out: &mut Output<'_>,
    ) -> StepRecord {
        out.number = number;
        if self.cancel.is_cancelled() {
            self.status = JobStatus::Cancelled;
        }
        let env = self.ctx.expr_env(self.status, &[]);
        let name = out.masker.mask(&step_name(step, &env)).into_owned();
        let mut record = StepRecord {
            number,
            context_id: context_id(step).map(str::to_owned),
            name,
            outcome: StepResult::Skipped,
            conclusion: StepResult::Skipped,
            summary_path: None,
        };
        let condition = if step.enabled {
            evaluate_condition(step.condition.as_deref().unwrap_or_default(), &env)
        } else {
            Ok(false)
        };
        let mut outputs = Vec::new();
        if !matches!(condition, Ok(false)) {
            out.sink.step_started(number, &record.name);
            let continue_on_error = continue_on_error(step, &env, out);
            let executed = match condition {
                Ok(_) => self.execute(step, &env, out).await,
                Err(error) => Err(error.into()),
            };
            let executed = executed.unwrap_or_else(|error| {
                out.message(&error.to_string());
                Executed {
                    result: StepResult::Failed,
                    outputs: Vec::new(),
                    summary: None,
                }
            });
            outputs = executed.outputs;
            record.summary_path = executed.summary;
            record.outcome = executed.result;
            record.conclusion = if executed.result == StepResult::Failed && continue_on_error {
                StepResult::Succeeded
            } else {
                executed.result
            };
        }
        out.sink.step_finished(number, record.outcome);
        self.settle(&record, outputs);
        record
    }

    fn settle(&mut self, record: &StepRecord, outputs: Vec<(String, String)>) {
        match record.conclusion {
            StepResult::Failed if self.status == JobStatus::Success => {
                self.status = JobStatus::Failure;
            }
            StepResult::Cancelled => self.status = JobStatus::Cancelled,
            _ => {}
        }
        if let Some(id) = &record.context_id {
            self.ctx.record_step(
                id,
                StepEntry {
                    outcome: record.outcome,
                    conclusion: record.conclusion,
                    outputs,
                },
            );
        }
    }

    async fn execute(
        &mut self,
        step: &ActionStep,
        env: &ExprEnv,
        out: &mut Output<'_>,
    ) -> Result<Executed, ExecError> {
        let step_env = match step.environment.as_ref() {
            Some(environment) => token::mapping(environment, env)?,
            None => Vec::new(),
        };
        let env = self.ctx.expr_env(self.status, &step_env);
        let minutes = step
            .timeout_in_minutes
            .as_ref()
            .map(|token| token::scalar(token, &env))
            .transpose()?
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|minutes| minutes.is_finite() && *minutes > 0.0);
        let cancel = (!self.cancel.is_cancelled()).then(|| self.cancel.clone());
        let files = FileCommands::create(&self.options.temp).await?;
        let process_env = self.ctx.process_env(&step_env, &files.variables());
        let ran = if let Some(reference) = reference(step) {
            self.run_adapter(step, &reference, &env, &process_env, cancel, out)
                .await
        } else {
            self.run_script_step(step, &env, process_env, minutes, cancel.as_ref(), out)
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
        minutes: Option<f64>,
        cancel: Option<&JobCancel>,
        out: &mut Output<'_>,
    ) -> Result<(StepResult, BTreeMap<String, String>), ExecError> {
        let script = ScriptStep {
            script: token::input(step.inputs.as_ref(), "script", env)?.unwrap_or_default(),
            shell: shell::resolve(step, &self.ctx, env)?,
            dir: self.working_directory(step, env)?,
            env: process_env,
            minutes,
        };
        let result = run_script(&script, &self.options.temp, cancel, out).await?;
        Ok((result, BTreeMap::new()))
    }

    fn working_directory(
        &self,
        step: &ActionStep,
        env: &ExprEnv,
    ) -> Result<std::path::PathBuf, ExecError> {
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
        cancel: Option<JobCancel>,
        out: &mut Output<'_>,
    ) -> Result<(StepResult, BTreeMap<String, String>), ExecError> {
        let adapter = self
            .adapters
            .find(reference)
            .ok_or_else(|| ExecError::Unsupported(format!("uses {reference}")))?;
        let inputs = match step.inputs.as_ref() {
            Some(inputs) => token::mapping(inputs, env)?.into_iter().collect(),
            None => BTreeMap::new(),
        };
        let mut log = |text: &str| out.line(text);
        let mut context = StepContext {
            reference: reference.to_owned(),
            inputs,
            env: process_env.iter().cloned().collect(),
            workspace: &self.options.workspace,
            temp: &self.options.temp,
            cancel,
            outputs: BTreeMap::new(),
            log: &mut log,
        };
        let result = adapter.run(&mut context).await?;
        Ok((result, context.outputs))
    }

    async fn finish(
        &mut self,
        result: StepResult,
        adapter_outputs: BTreeMap<String, String>,
        collected: Result<Collected, ExecError>,
        out: &mut Output<'_>,
    ) -> Result<Executed, ExecError> {
        let collected = match collected {
            Ok(collected) => collected,
            Err(error) => {
                out.message(&error.to_string());
                return Ok(Executed {
                    result: StepResult::Failed,
                    outputs: adapter_outputs.into_iter().collect(),
                    summary: None,
                });
            }
        };
        let mut writes = Vec::new();
        for (name, value) in collected.env {
            if name.eq_ignore_ascii_case("NODE_OPTIONS") {
                out.annotations.push(Annotation::new(
                    AnnotationLevel::Error,
                    "Can't store NODE_OPTIONS output parameter using '$GITHUB_ENV' command.",
                ));
            } else {
                writes.push((name, value));
            }
        }
        self.ctx.apply_env(&writes);
        self.ctx.prepend_path(&collected.path);
        let mut outputs: Vec<(String, String)> = adapter_outputs.into_iter().collect();
        for (name, value) in &collected.outputs {
            set_pair(&mut outputs, name, value);
        }
        let summary = save_summary(
            &self.options.temp,
            out.number,
            &collected.summary,
            out.masker,
            out.annotations,
        )
        .await?;
        Ok(Executed {
            result,
            outputs,
            summary,
        })
    }
}
