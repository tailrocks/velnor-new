// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanPhaseTimings {
    pub plan_internal_us: u128,
    pub prepare_us: u128,
    pub metadata_commands: u64,
    pub metadata_run_us: u128,
    pub metadata_parse_us: u128,
    pub generator_sha_calls: u64,
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
