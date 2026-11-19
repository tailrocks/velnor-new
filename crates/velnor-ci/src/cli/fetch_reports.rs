// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct FetchReportsArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "Workflow run whose artifacts hold the reports")]
    pub run_id: u64,
    #[arg(
        long,
        help = "Run directory: plan.json is read from it and every report artifact is extracted to <run-dir>/reports/<artifact name>"
    )]
    pub run_dir: PathBuf,
    #[arg(
        long,
        default_value_t = 16,
        value_parser = clap::value_parser!(u8).range(1..=32),
        help = "Artifacts downloaded at the same time"
    )]
    pub connections: u8,
}
