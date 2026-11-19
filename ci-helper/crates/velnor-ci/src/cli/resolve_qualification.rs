// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct ResolveQualificationArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "The plan request `write-request` wrote; default $RUNNER_TEMP/velnor/request/plan-v1-request.json, the only path accepted"
    )]
    pub request: Option<PathBuf>,
    #[arg(
        long,
        value_name = "BRANCH",
        help = "The repository's default branch the request is checked against; default origin/HEAD, then repository.default_branch of the event payload"
    )]
    pub default_branch: Option<String>,
}
