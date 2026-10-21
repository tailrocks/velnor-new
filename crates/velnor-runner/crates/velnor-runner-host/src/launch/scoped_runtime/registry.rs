//! Retain live Runner state until its exact native start event arrives.

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

    /// Drop a ready value only after its journal cleanup proof is observed.
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

#[cfg(test)]
mod tests {
    use super::RunnerReadyRegistry;

    #[test]
    fn exact_started_event_takes_one_retained_runner() {
        let mut registry = RunnerReadyRegistry::<&str>::new();
        assert_eq!(registry.insert(41, "runner-41"), Ok(()));

        assert_eq!(registry.record_started([42, 41, 41, -1]), [41]);
        assert_eq!(registry.take_started(41), Some("runner-41"));
        assert_eq!(registry.record_started([41]), [] as [i64; 0]);
        assert_eq!(registry.take_started(41), None);
    }

    #[test]
    fn duplicate_request_keeps_both_values_without_replacing_live_state() {
        let mut registry = RunnerReadyRegistry::<&str>::new();
        assert_eq!(registry.insert(41, "original"), Ok(()));

        assert_eq!(registry.insert(41, "duplicate"), Err("duplicate"));
        assert_eq!(registry.record_started([41]), [41]);
        assert_eq!(registry.take_started(41), Some("original"));
    }

    #[test]
    fn live_request_id_is_released_after_terminal_cleanup() {
        let mut registry = RunnerReadyRegistry::<&str>::new();
        assert_eq!(registry.insert(41, "original"), Ok(()));
        assert_eq!(registry.record_started([41]), [41]);
        assert!(!registry.retire(41));

        assert_eq!(registry.take_started(41), Some("original"));
        assert_eq!(registry.insert(41, "replay"), Err("replay"));
        assert!(registry.retire(41));

        // New acquisitions pass the durable assignment replay guard first.
        assert_eq!(registry.insert(42, "new assignment"), Ok(()));
    }

    #[test]
    fn unstarted_ready_value_is_released_only_through_cleanup_path() {
        let mut registry = RunnerReadyRegistry::<&str>::new();
        assert_eq!(registry.insert(41, "runner-41"), Ok(()));
        assert!(!registry.is_empty());
        assert_eq!(registry.take_ready_after_cleanup(41), Some("runner-41"));
        assert!(registry.is_empty());
        assert_eq!(registry.insert(42, "new assignment"), Ok(()));
    }
}
