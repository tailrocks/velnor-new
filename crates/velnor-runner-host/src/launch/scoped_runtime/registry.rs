// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::jit::RunnerReadyBootstrap;

#[must_use]
pub(crate) struct RunnerReadyRegistry<T = RunnerReadyBootstrap> {
    ready: BTreeMap<i64, T>,
    started: BTreeMap<i64, T>,
    seen: BTreeSet<i64>,
}

impl<T> RunnerReadyRegistry<T> {
    pub(crate) fn new() -> Self {
        Self {
            ready: BTreeMap::new(),
            started: BTreeMap::new(),
            seen: BTreeSet::new(),
        }
    }

    pub(crate) fn insert(&mut self, request_id: i64, runner: T) -> Result<(), T> {
        if request_id <= 0 || !self.seen.insert(request_id) {
            return Err(runner);
        }
        self.ready.insert(request_id, runner);
        Ok(())
    }

    pub(crate) fn record_started(
        &mut self,
        request_ids: impl IntoIterator<Item = i64>,
    ) -> Vec<i64> {
        let mut unique = BTreeSet::new();
        request_ids
            .into_iter()
            .filter(|request_id| *request_id > 0 && unique.insert(*request_id))
            .filter_map(|request_id| {
                let runner = self.ready.remove(&request_id)?;
                self.started.insert(request_id, runner);
                Some(request_id)
            })
            .collect()
    }

    pub(crate) fn take_started(&mut self, request_id: i64) -> Option<T> {
        self.started.remove(&request_id)
    }

    pub(crate) fn take_ready_after_cleanup(&mut self, request_id: i64) -> Option<T> {
        if self.started.contains_key(&request_id) {
            return None;
        }
        let ready = self.ready.remove(&request_id)?;
        self.seen.remove(&request_id);
        Some(ready)
    }

    pub(crate) fn retire(&mut self, request_id: i64) -> bool {
        if self.ready.contains_key(&request_id) || self.started.contains_key(&request_id) {
            return false;
        }
        self.seen.remove(&request_id)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ready.is_empty() && self.started.is_empty()
    }
}

impl Default for RunnerReadyRegistry<RunnerReadyBootstrap> {
    fn default() -> Self {
        Self::new()
    }
}
