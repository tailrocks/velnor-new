// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::CommandFactory;
use velnor_ci::cli::Cli;

#[test]
fn the_commands_a_declared_workflow_may_run_are_exactly_the_cli_subcommands() {
    let mut cli: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_owned())
        .collect();
    let mut declared: Vec<String> = velnor_gen::HELPER_COMMANDS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    cli.sort();
    declared.sort();
    assert_eq!(declared, cli);
}
