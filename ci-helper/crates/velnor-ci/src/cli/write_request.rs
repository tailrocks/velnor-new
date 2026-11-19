// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct WriteRequestArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Where to write the request; default $RUNNER_TEMP/velnor/request/plan-v1-request.json. Must lie under $RUNNER_TEMP and must not exist"
    )]
    pub output: Option<PathBuf>,
    #[arg(
        long,
        value_name = "NAME",
        help = "The event, default $GITHUB_EVENT_NAME"
    )]
    pub event_name: Option<String>,
    #[arg(
        long,
        value_name = "FILE",
        help = "The event payload file, default $GITHUB_EVENT_PATH"
    )]
    pub event_path: Option<PathBuf>,
    #[arg(long, value_name = "SHA", help = "Default $GITHUB_SHA")]
    pub sha: Option<String>,
    #[arg(long, value_name = "OWNER/NAME", help = "Default $GITHUB_REPOSITORY")]
    pub repository: Option<String>,
    #[arg(long = "ref", value_name = "REF", help = "Default $GITHUB_REF")]
    pub git_ref: Option<String>,
    #[arg(long, value_name = "BOOL", help = "Default $GITHUB_REF_PROTECTED")]
    pub ref_protected: Option<String>,
    #[arg(long, value_name = "REF", help = "Default $GITHUB_WORKFLOW_REF")]
    pub workflow_ref: Option<String>,
    #[arg(long, value_name = "SHA", help = "Default $GITHUB_WORKFLOW_SHA")]
    pub workflow_sha: Option<String>,
    #[arg(long, value_name = "ID", help = "Default $GITHUB_RUN_ID")]
    pub run_id: Option<String>,
    #[arg(long, value_name = "N", help = "Default $GITHUB_RUN_ATTEMPT")]
    pub run_attempt: Option<String>,
    #[arg(
        long,
        value_name = "JSON",
        help = "The named check lanes: an object of job id to a list of {job_id, variant?}"
    )]
    pub named_check_lanes: Option<String>,
    #[arg(long, value_name = "JSON", help = "The lane inventory: {mode, routes}")]
    pub lane_inventory: Option<String>,
}
