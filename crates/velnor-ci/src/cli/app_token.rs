// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Args;

use crate::app_token::Permission;

#[derive(Debug, Args)]
pub struct AppTokenArgs {
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
