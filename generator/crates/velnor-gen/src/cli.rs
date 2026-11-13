// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "velnor-gen",
    version,
    about = "Generate the Rust CI workflow of a repository"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        default_value = ".",
        help = "A path inside the repository"
    )]
    pub root: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum Command {
    #[command(about = "Write the generated files and remove the ones no longer generated")]
    Generate,
    #[command(about = "Exit 1 when a generated file differs from what generate would write")]
    Check,
}
