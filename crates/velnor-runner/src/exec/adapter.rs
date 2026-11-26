// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use futures_util::future::BoxFuture;

use super::step::{Output, deadline, report_timeout};
use super::{ExecError, JobCancel, StepResult};

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
    pub inputs: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub workspace: &'a Path,
    pub temp: &'a Path,
    pub cancel: Option<JobCancel>,
    pub(super) outputs: BTreeMap<String, String>,
    pub(super) log: &'a mut (dyn FnMut(&str) + Send),
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
        (self.log)(text);
    }

    pub fn set_output(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.outputs.insert(name.into(), value.into());
    }
}

pub(super) struct AdapterCall<'a> {
    pub(super) reference: String,
    pub(super) inputs: BTreeMap<String, String>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) workspace: &'a Path,
    pub(super) temp: &'a Path,
    pub(super) cancel: Option<JobCancel>,
    pub(super) minutes: Option<f64>,
}

pub(super) async fn invoke(
    adapter: &dyn StepAdapter,
    call: AdapterCall<'_>,
    out: &mut Output<'_>,
) -> Result<(StepResult, BTreeMap<String, String>), ExecError> {
    let minutes = call.minutes;
    let mut log = |text: &str| out.line(text);
    let mut context = StepContext {
        reference: call.reference,
        inputs: call.inputs,
        env: call.env,
        workspace: call.workspace,
        temp: call.temp,
        cancel: call.cancel,
        outputs: BTreeMap::new(),
        log: &mut log,
    };
    let ran = match deadline(minutes) {
        Some(limit) => tokio::time::timeout(limit, adapter.run(&mut context))
            .await
            .ok(),
        None => Some(adapter.run(&mut context).await),
    };
    let outputs = std::mem::take(&mut context.outputs);
    drop(context);
    let Some(result) = ran else {
        report_timeout(out, minutes);
        return Ok((StepResult::Failed, outputs));
    };
    Ok((result?, outputs))
}
