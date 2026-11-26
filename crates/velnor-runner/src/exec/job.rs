// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::adapter::AdapterRegistry;
use super::cancel::Groups;
use super::commands;
use super::context::{JobContext, StepEntry};
use super::contexts::set_output;
use super::evaluate;
use super::execute::Executed;
use super::issues::Issues;
use super::mask::{self, Masker};
use super::output::Output;
use super::preflight::problems;
use super::sink::LogSink;
use super::step_info::{context_id, continue_on_error, step_environment, step_name};
use super::{
    AnnotationLevel, ExecError, JobCancel, JobResult, JobRun, RunOptions, StepRecord, StepResult,
};
use crate::expr::JobStatus;
use crate::job_message::{ActionStep, AgentJobRequestMessage};

pub(super) struct Job<'a> {
    pub(super) options: &'a RunOptions,
    pub(super) adapters: &'a AdapterRegistry,
    pub(super) groups: Groups,
    pub(super) ctx: JobContext,
    pub(super) message: &'a AgentJobRequestMessage,
    pub(super) cancel: JobCancel,
    status: JobStatus,
}

fn reject(out: &mut Output<'_>, lines: &[String]) {
    for line in lines {
        out.message(line);
        out.annotate(AnnotationLevel::Error, line.clone());
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
    let mut masker = Masker::default();
    let loaded = mask::load(&mut masker, message);
    let mut issues = Issues::default();
    let mut out = Output {
        number: 0,
        sink,
        masker: &mut masker,
        issues: &mut issues,
    };
    let started = match loaded {
        Ok(()) => JobContext::new(message, options).await,
        Err(error) => Err(error),
    };
    match started {
        Err(error) => reject(&mut out, &[error.to_string()]),
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
                    groups: Groups::default(),
                    ctx,
                    status: JobStatus::Success,
                };
                job.execute_all(&mut out, &mut run).await;
            } else {
                reject(&mut out, &unsupported);
            }
        }
    }
    run.annotations = issues.into_items();
    let workspace = options.workspace.to_string_lossy();
    for annotation in &mut run.annotations {
        commands::relativize(annotation, &workspace);
        mask::mask_annotation(&masker, annotation);
    }
    let logged = JobRun {
        outputs: run
            .outputs
            .iter()
            .map(|(name, value)| (masker.mask(name).into_owned(), value.clone()))
            .collect(),
        ..run.clone()
    };
    sink.job_finished(&logged);
    run
}

impl Job<'_> {
    async fn execute_all(&mut self, out: &mut Output<'_>, run: &mut JobRun) {
        for (index, step) in self.message.steps.iter().enumerate() {
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let record = self.run_step(number, step, out).await;
            run.steps.push(record);
        }
        self.collect_outputs(out, run).await;
        self.groups.kill_all();
        run.result = match self.status {
            JobStatus::Success => JobResult::Succeeded,
            JobStatus::Failure => JobResult::Failed,
            JobStatus::Cancelled => JobResult::Cancelled,
        };
    }

    async fn collect_outputs(&mut self, out: &mut Output<'_>, run: &mut JobRun) {
        let Some(token) = self.message.job_outputs.as_ref() else {
            return;
        };
        out.enter(0);
        let mapped = match self.ctx.expr_env(self.status, &[]) {
            Ok(env) => evaluate::mapping(token, &env)
                .await
                .map_err(ExecError::from),
            Err(error) => Err(error),
        };
        match mapped {
            Ok(outputs) => {
                let mut unique = Vec::new();
                for (name, value) in &outputs {
                    set_output(&mut unique, name, value);
                }
                for (name, value) in unique.into_iter().filter(|(_, value)| !value.is_empty()) {
                    if out.masker.reveals(&value) {
                        let warning = format!("Skip output '{name}' since it may contain secret.");
                        out.message(&warning);
                        out.annotate(AnnotationLevel::Warning, warning);
                    } else {
                        run.outputs.insert(name, value);
                    }
                }
            }
            Err(error) => {
                out.message(&error.to_string());
                out.annotate(AnnotationLevel::Error, error.to_string());
                if self.status == JobStatus::Success {
                    self.status = JobStatus::Failure;
                }
            }
        }
    }

    async fn forgives(
        &self,
        step: &ActionStep,
        vars: &[(String, String)],
        out: &mut Output<'_>,
    ) -> bool {
        match self.ctx.expr_env(self.status, vars) {
            Ok(env) => continue_on_error(step, &env, out).await,
            Err(error) => {
                out.message(&error.to_string());
                false
            }
        }
    }

    async fn run_step(
        &mut self,
        number: u32,
        step: &ActionStep,
        out: &mut Output<'_>,
    ) -> StepRecord {
        out.enter(number);
        let already_cancelled = self.cancel.is_cancelled();
        if already_cancelled {
            self.status = JobStatus::Cancelled;
        }
        let prepared = self.ctx.begin_step(step).await;
        let step_env = match self.ctx.expr_env(self.status, &[]) {
            Ok(env) => step_environment(step, &env).await,
            Err(error) => Err(error),
        };
        let step_vars = step_env.as_deref().unwrap_or_default().to_vec();
        let (env, step_env) = match self.ctx.expr_env(self.status, &step_vars) {
            Ok(env) => (env, step_env),
            Err(error) => (self.ctx.base_env(), step_env.and(Err(error))),
        };
        let name = out.masker.mask(&step_name(step, &env).await).into_owned();
        let mut record = StepRecord {
            number,
            context_id: context_id(step).map(str::to_owned),
            name,
            outcome: StepResult::Skipped,
            conclusion: StepResult::Skipped,
            summary_path: None,
        };
        let condition_text = step.condition.as_deref().unwrap_or_default();
        let condition = match (step.enabled, &step_env) {
            (false, _) => Ok(false),
            (true, Err(_)) => Ok(true),
            (true, Ok(_)) => evaluate::condition(condition_text, &env).await,
        };
        let condition_failed = condition.is_err() || step_env.is_err();
        let mut outputs = Vec::new();
        let annotations_before = out.annotations.len();
        if !matches!(condition, Ok(false)) {
            out.sink.step_started(number, &record.name);
            let executed = match (condition, prepared, step_env) {
                (Ok(_), Ok(()), Ok(step_env)) => {
                    self.execute_watched(step, step_env, &env, &step_vars, already_cancelled, out)
                        .await
                }
                (Err(error), _, _) => Err(error.into()),
                (_, Err(error), _) | (_, _, Err(error)) => Err(error),
            };
            let executed = executed.unwrap_or_else(|error| {
                out.message(&error.to_string());
                out.annotate(AnnotationLevel::Error, error.to_string());
                Executed {
                    result: StepResult::Failed,
                    outputs: Vec::new(),
                    summary: None,
                    env: Vec::new(),
                }
            });
            outputs = executed.outputs;
            record.summary_path = executed.summary;
            record.outcome = executed.result;
            let forgiven = executed.result == StepResult::Failed
                && !condition_failed
                && self
                    .forgives(
                        step,
                        &[step_vars.as_slice(), executed.env.as_slice()].concat(),
                        out,
                    )
                    .await;
            record.conclusion = if forgiven {
                StepResult::Succeeded
            } else {
                executed.result
            };
        }
        if self.cancel.is_cancelled() {
            self.status = JobStatus::Cancelled;
        }
        for annotation in &mut out.annotations[annotations_before..] {
            annotation.step = Some(number);
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
}
