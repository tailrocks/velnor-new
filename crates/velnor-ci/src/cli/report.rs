// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct ReportArgs {
    #[arg(long, help = "Directory of downloaded lane metrics to check")]
    pub check: Option<PathBuf>,
}
