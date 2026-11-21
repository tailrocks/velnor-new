// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Args;

#[derive(Debug, Args)]
pub struct AppTokenArgs {
    #[arg(long, value_name = "OWNER", help = "The owner of the repositories")]
    pub owner: String,
    #[arg(
        long = "repository",
        value_name = "NAME",
        required = true,
        help = "A repository the token may read, by name without the owner; repeat for more"
    )]
    pub repositories: Vec<String>,
}
