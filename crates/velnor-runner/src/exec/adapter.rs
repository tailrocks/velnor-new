// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use futures_util::future::BoxFuture;

use super::cancel::{Groups, KILL_GRACE};
use super::contexts::set_output;
use super::output::Output;
use super::step::{Limits, ProcessSpec, capture, deadline, report_timeout, run};
use super::{ExecError, Inputs, JobCancel, StepResult};

const CANCEL_GRACE: Duration = Duration::from_secs(KILL_GRACE.as_secs() + 1);

pub type StepFuture<'a> = BoxFuture<'a, Result<StepResult, ExecError>>;

pub trait StepAdapter: Send + Sync {
    fn handles(&self, reference: &str) -> bool;

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
    pub workspace: &'a Path,
    pub temp: &'a Path,
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

impl StepContext<'_> {
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
        };
        let limits = Limits {
            minutes: None,
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
        };
        let limits = Limits {
            minutes: None,
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
    pub(super) workspace: &'a Path,
    pub(super) temp: &'a Path,
    pub(super) cancel: Option<JobCancel>,
    pub(super) minutes: Option<f64>,
    pub(super) groups: &'a Groups,
}

#[derive(Clone, Copy)]
enum Interruption {
    Cancelled,
    TimedOut,
}

async fn interrupted(cancel: Option<&JobCancel>, minutes: Option<f64>) -> Interruption {
    let cancelled = async {
        match cancel {
            Some(cancel) => cancel.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let timer = async {
        match deadline(minutes) {
            Some(limit) => tokio::time::sleep(limit).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        () = cancelled => Interruption::Cancelled,
        () = timer => Interruption::TimedOut,
    }
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
        workspace: call.workspace,
        temp: call.temp,
        cancel: Some(step_cancel.clone()),
        outputs: Vec::new(),
        groups: call.groups,
        out: &mut inner,
    };
    let (finished, why) = {
        let running = adapter.run(&mut context);
        tokio::pin!(running);
        tokio::select! {
            biased;
            result = &mut running => (Some(result), None),
            why = interrupted(call.cancel.as_ref(), call.minutes) => {
                step_cancel.cancel();
                let settled = tokio::time::timeout(CANCEL_GRACE, &mut running).await.ok();
                (settled, Some(why))
            }
        }
    };
    let outputs = std::mem::take(&mut context.outputs);
    drop(context);
    match (why, finished) {
        (None, Some(result)) => Ok((result?, outputs)),
        (Some(Interruption::TimedOut), _) => {
            report_timeout(out, call.minutes);
            Ok((StepResult::Failed, outputs))
        }
        _ => Ok((StepResult::Cancelled, outputs)),
    }
}
