// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use futures_util::future::BoxFuture;

use super::cancel::{Groups, KILL_GRACE};
use super::contexts::set_output;
use super::output::Output;
use super::step::{Limits, ProcessSpec, capture, run, run_isolated};
use super::timeout::{Timeout, interrupted, stop_result};
use super::{AnnotationLevel, ExecError, Inputs, JobCancel, JobFacts, StepResult};

const CANCEL_GRACE: Duration = Duration::from_secs(KILL_GRACE.as_secs() + 1);

pub type StepFuture<'a> = BoxFuture<'a, Result<StepResult, ExecError>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputValue {
    Literal(String),
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredInputs {
    Known(BTreeMap<String, InputValue>),
    Unknown,
}

pub trait StepAdapter: Send + Sync {
    fn handles(&self, reference: &str) -> bool;

    fn problem(&self, _reference: &str, _inputs: &DeclaredInputs) -> Option<String> {
        None
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a>;
}

#[derive(Default)]
pub struct AdapterRegistry {
    adapters: Vec<Box<dyn StepAdapter>>,
}

impl std::fmt::Debug for AdapterRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdapterRegistry")
            .field("adapters", &self.adapters.len())
            .finish()
    }
}

impl AdapterRegistry {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Box<dyn StepAdapter>) {
        self.adapters.push(adapter);
    }

    #[must_use]
    pub fn find(&self, reference: &str) -> Option<&dyn StepAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.handles(reference))
            .map(AsRef::as_ref)
    }
}

pub struct StepContext<'a> {
    pub reference: String,
    pub inputs: Inputs,
    pub env: BTreeMap<String, String>,
    pub job: &'a JobFacts,
    pub workspace: &'a Path,
    pub temp: &'a Path,
    pub github_token: Option<String>,
    pub cancel: Option<JobCancel>,
    pub(super) outputs: Vec<(String, String)>,
    pub(super) groups: &'a Groups,
    pub(super) out: &'a mut Output<'a>,
}

impl std::fmt::Debug for StepContext<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StepContext")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl StepContext<'_> {
    pub(crate) async fn run_isolated(
        &mut self,
        spec: (&str, &[String], &Path, &[(String, String)]),
        quiet: bool,
    ) -> Result<ProcessOutcome, ExecError> {
        let (program, args, dir, env) = spec;
        let spec = ProcessSpec {
            program,
            args,
            dir,
            env,
            isolated: true,
        };
        let limits = Limits {
            timeout: None,
            cancel: self.cancel.as_ref(),
            groups: self.groups,
        };
        run_isolated(&spec, &limits, (!quiet).then_some(&mut *self.out)).await
    }

    pub fn log(&mut self, text: &str) {
        self.out.line(text);
    }

    pub fn set_output(&mut self, name: impl Into<String>, value: impl Into<String>) {
        set_output(&mut self.outputs, &name.into(), &value.into());
    }

    #[must_use]
    pub fn env_with(&self, extra: &[(String, String)]) -> Vec<(String, String)> {
        let mut merged = self.env.clone();
        merged.extend(extra.iter().cloned());
        merged.into_iter().collect()
    }

    pub async fn run_process(
        &mut self,
        program: &str,
        args: &[String],
        extra_env: &[(String, String)],
    ) -> Result<StepResult, ExecError> {
        let env = self.env_with(extra_env);
        let spec = ProcessSpec {
            program,
            args,
            dir: self.workspace,
            env: &env,
            isolated: false,
        };
        let limits = Limits {
            timeout: None,
            cancel: self.cancel.as_ref(),
            groups: self.groups,
        };
        run(&spec, &limits, self.out).await
    }

    pub async fn capture(
        &mut self,
        program: &str,
        args: &[String],
        extra_env: &[(String, String)],
    ) -> Result<String, ExecError> {
        let env = self.env_with(extra_env);
        let spec = ProcessSpec {
            program,
            args,
            dir: self.workspace,
            env: &env,
            isolated: false,
        };
        let limits = Limits {
            timeout: None,
            cancel: self.cancel.as_ref(),
            groups: self.groups,
        };
        capture(&spec, &limits).await
    }
}

pub(super) struct AdapterCall<'a> {
    pub(super) reference: String,
    pub(super) inputs: Inputs,
    pub(super) env: BTreeMap<String, String>,
    pub(super) job: &'a JobFacts,
    pub(super) workspace: &'a Path,
    pub(super) temp: &'a Path,
    pub(super) github_token: Option<String>,
    pub(super) cancel: Option<JobCancel>,
    pub(super) timeout: Option<Timeout>,
    pub(super) groups: &'a Groups,
}

pub(super) async fn invoke(
    adapter: &dyn StepAdapter,
    call: AdapterCall<'_>,
    out: &mut Output<'_>,
) -> Result<(StepResult, Vec<(String, String)>), ExecError> {
    let step_cancel = JobCancel::new();
    let mut inner = Output {
        number: out.number,
        sink: &mut *out.sink,
        masker: &mut *out.masker,
        issues: &mut *out.issues,
    };
    let mut context = StepContext {
        reference: call.reference,
        inputs: call.inputs,
        env: call.env,
        job: call.job,
        workspace: call.workspace,
        temp: call.temp,
        github_token: call.github_token,
        cancel: Some(step_cancel.clone()),
        outputs: Vec::new(),
        groups: call.groups,
        out: &mut inner,
    };
    let finished = {
        let running = adapter.run(&mut context);
        tokio::pin!(running);
        tokio::select! {
            biased;
            result = &mut running => Some(result),
            () = interrupted(call.cancel.as_ref(), call.timeout) => {
                step_cancel.cancel();
                let _ = tokio::time::timeout(CANCEL_GRACE, &mut running).await;
                None
            }
        }
    };
    let outputs = std::mem::take(&mut context.outputs);
    drop(context);
    match finished {
        Some(Ok(result)) => Ok((result, outputs)),
        Some(Err(error)) => {
            out.message(&error.to_string());
            out.annotate(AnnotationLevel::Error, error.to_string());
            Ok((StepResult::Failed, outputs))
        }
        None => Ok((stop_result(call.cancel.as_ref()), outputs)),
    }
}
