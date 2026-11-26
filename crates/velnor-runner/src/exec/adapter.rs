// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use futures_util::future::BoxFuture;

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
