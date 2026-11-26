// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "velnor-runner",
    version,
    about = "Velnor self-hosted GitHub Actions runner"
)]
struct Cli;

fn main() {
    Cli::parse();
}
