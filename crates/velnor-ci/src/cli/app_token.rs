// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::{Args, Subcommand};

use crate::app_token::Permission;

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct AppTokenArgs {
    #[command(subcommand)]
    pub handshake: Option<Handshake>,
    #[command(flatten)]
    pub mint: Option<MintArgs>,
}

#[derive(Debug, Clone, Args)]
pub struct MintArgs {
    #[arg(long, value_name = "OWNER", help = "The owner of the repositories")]
    pub owner: String,
    #[arg(
        long = "repository",
        value_name = "NAME",
        required = true,
        help = "A repository the token may reach, by name without the owner; repeat for more"
    )]
    pub repositories: Vec<String>,
    #[arg(
        long = "permission",
        value_name = "NAME:ACCESS",
        default_value = "contents:read",
        help = "A permission of the token, named as the GitHub API names it (lower-case letters and _, such as pull_requests) with read or write; repeat for more"
    )]
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Args)]
pub struct JobArgs {
    #[arg(
        long,
        value_name = "ID",
        help = "The identity of the job that wants the token: the job id of the workflow, with the matrix index appended for a matrix job; the relay job must pass the same value"
    )]
    pub job: String,
}

#[derive(Debug, Args)]
pub struct WaitArgs {
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 900,
        value_parser = clap::value_parser!(u64).range(1..=3600),
        help = "How long to wait before the command fails"
    )]
    pub timeout_seconds: u64,
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 5,
        value_parser = clap::value_parser!(u64).range(1..=60),
        help = "How long to pause between two looks at the artifact list"
    )]
    pub poll_seconds: u64,
}

#[derive(Debug, Args)]
pub struct RelayArgs {
    #[command(flatten)]
    pub job: JobArgs,
    #[command(flatten)]
    pub mint: MintArgs,
    #[command(flatten)]
    pub wait: WaitArgs,
}

#[derive(Debug, Args)]
pub struct AwaitArgs {
    #[command(flatten)]
    pub job: JobArgs,
    #[command(flatten)]
    pub wait: WaitArgs,
}

#[derive(Debug, Subcommand)]
pub enum Handshake {
    #[command(
        about = "Run in the job that wants the token, as its first step after installing velnor-ci: generate a throwaway X25519 key pair, keep the private half in a 0600 file under $RUNNER_TEMP that the await command deletes, write the public half bound to $GITHUB_REPOSITORY, $GITHUB_RUN_ID, $GITHUB_RUN_ATTEMPT and --job to $RUNNER_TEMP/velnor-app-token/<job>/public-key.json, and write the step outputs artifact (the name to upload it under) and path"
    )]
    Request(JobArgs),
    #[command(
        about = "Run in a job that checks out no repository and runs no repository code: wait up to --timeout-seconds for the public key artifact of --job in this run (GET /repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/artifacts with GITHUB_TOKEN), refuse one bound to another repository, run, attempt or job, create the installation token as app-token does with the PEM key in VELNOR_APP_PRIVATE_KEY, seal it to the public key, write the sealed token to $RUNNER_TEMP/velnor-app-token/<job>/token.sealed, and write the step outputs artifact (the name to upload it under) and path. The token is never written in clear"
    )]
    Relay(RelayArgs),
    #[command(
        about = "Run in the job that wants the token, after the public key artifact is uploaded: wait up to --timeout-seconds for the sealed token artifact of --job in this run, open it with the private key of the request command, refuse one bound to another repository, run, attempt or job, delete the private key, and write the token to $GITHUB_OUTPUT as token after an ::add-mask:: line. A later step revokes the token with revoke-token"
    )]
    Await(AwaitArgs),
}
