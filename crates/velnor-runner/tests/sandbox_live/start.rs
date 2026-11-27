// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use super::support::Sandbox;

const RUNS: usize = 21;
const BUDGET: Duration = Duration::from_millis(100);

#[test]
fn the_sandbox_starts_in_under_100_ms_at_the_median() {
    let sandbox = Sandbox::new("latency");
    sandbox.run(&["/bin/true"]);
    let mut times: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let started = Instant::now();
            let output = sandbox.run(&["/bin/true"]);
            assert!(output.status.success());
            started.elapsed()
        })
        .collect();
    times.sort();
    assert!(
        times[RUNS / 2] < BUDGET,
        "median {:?} of {times:?}; run the lane with --release",
        times[RUNS / 2]
    );
}
