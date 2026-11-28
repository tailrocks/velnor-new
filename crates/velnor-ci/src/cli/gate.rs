// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Args;

#[derive(Debug, Args)]
pub struct GateArgs {
    #[arg(
        long,
        value_name = "VARIABLE",
        help = "The environment variable that holds the gate spec the generator wrote: --plan <job>, then --job <ID=WHEN> for every job of needs (WHEN is always, or conditions joined by +: lane:<root>, group:<root>:<name>, policy, schedule, schedule-or-push) and --tests <ID=ROOT> for each job whose tests_run output counts the tests of the root; --reuse <ID> names a job whose skipped result a reused verdict may stand in for, and --reuse-tests <ID=ROOT:STEP,STEP;ROOT:STEP> names, per root, the reusable test steps whose absent counts it may stand in for. On a merge_group event, when the plan job output reuse is exactly true, a job or step whose id is in the space-delimited output reused is accepted without running, and each is printed as `reused: <id> passed in <reuse_run>`"
    )]
    pub spec_env: String,
}
