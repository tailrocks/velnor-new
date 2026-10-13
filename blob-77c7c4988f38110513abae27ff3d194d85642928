//! Per-call timing fields used only by the P13 benchmark collector.

use std::time::Instant;

/// Named nested phase measurements for one internal plan invocation.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanPhaseTimings {
    /// Full `plan_internal` wall for this one invocation.
    pub plan_internal_us: u128,
    /// Full `prepare` wall, including the nested metadata phases.
    pub prepare_us: u128,
    /// Number of pinned Cargo metadata subprocesses actually invoked.
    pub metadata_commands: u64,
    /// Aggregate wall spent waiting for those metadata subprocesses.
    pub metadata_run_us: u128,
    /// Aggregate metadata output decoding and JSON parse wall.
    pub metadata_parse_us: u128,
    /// Number of current-executable digest operations.
    pub generator_sha_calls: u64,
    /// Current-executable lookup, read, and SHA-256 wall.
    pub generator_sha_us: u128,
}

impl PlanPhaseTimings {
    pub(crate) fn measure_prepare<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        let started = Instant::now();
        let result = work(self);
        self.prepare_us = started.elapsed().as_micros();
        result
    }
}

/// Compute a plan and return per-call phase timings for the benchmark harness.
///
/// This diagnostic Rust API uses the same planner and response as
/// `plan_internal`; it adds no request field or CLI option.
#[doc(hidden)]
pub fn plan_internal_with_phase_timings(
    request_json: &str,
) -> Result<(String, PlanPhaseTimings), super::OrchestratorError> {
    let mut phases = PlanPhaseTimings::default();
    let started = Instant::now();
    let response = super::plan_internal_inner(request_json, Some(&mut phases));
    phases.plan_internal_us = started.elapsed().as_micros();
    Ok((response?, phases))
}
