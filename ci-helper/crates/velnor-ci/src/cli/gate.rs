// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Args;

#[derive(Debug, Args)]
pub struct GateArgs {
    #[arg(
        long,
        value_name = "VARIABLE",
        help = "The environment variable that holds the gate spec the generator wrote: --plan <job>, then --job <ID=WHEN> for every job of needs (WHEN is always, or conditions joined by +: lane:<root>, policy, schedule, schedule-or-push) and --tests <ID=ROOT> for each job whose tests_run output counts the tests of the root"
    )]
    pub spec_env: String,
}
