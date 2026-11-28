// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Stdio};

use super::support::{Sandbox, unstarted};

const RUNS: usize = 21;
const BUDGET_SECONDS: f64 = 0.1;
const TIMED: &str = "TIMEFORMAT='%U %S'; { time \"$@\" >/dev/null 2>/dev/null; } 2>&1";

fn cpu_seconds(sandbox: &Sandbox) -> f64 {
    let inner = sandbox.command(&["/bin/true"]);
    let output = unstarted(
        Command::new("bash")
            .args(["-c", TIMED, "bash"])
            .arg(inner.get_program())
            .args(inner.get_args())
            .stdin(Stdio::null())
            .output(),
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|field| field.parse::<f64>().ok())
        .sum()
}

#[test]
fn the_sandbox_parent_and_its_chain_use_under_100_ms_of_cpu_at_the_median() {
    let sandbox = Sandbox::new("latency");
    sandbox.run(&["/bin/true"]);
    let mut times: Vec<f64> = (0..RUNS).map(|_| cpu_seconds(&sandbox)).collect();
    times.sort_by(f64::total_cmp);
    assert!(
        times[RUNS / 2] > 0.0 && times[RUNS / 2] < BUDGET_SECONDS,
        "median {} s of {times:?}; run the lane with --release",
        times[RUNS / 2]
    );
}
