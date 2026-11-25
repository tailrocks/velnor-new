// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn without_open(command: &[String]) -> Vec<String> {
    let end = command
        .iter()
        .position(|word| word == "--")
        .unwrap_or(command.len());
    command[..end]
        .iter()
        .filter(|word| *word != "--open")
        .chain(&command[end..])
        .cloned()
        .collect()
}

pub(super) fn runs_doctests(arguments: &[&str], command: &[String]) -> bool {
    matches!(arguments, ["rustdoc", ..])
        && command
            .iter()
            .skip_while(|word| *word != "--")
            .any(|word| word == "--test")
}

pub(super) fn refusal(
    program: &str,
    arguments: &[&str],
    running: bool,
    building: bool,
    words: &[&str],
) -> Option<&'static str> {
    let names_target = words.iter().any(|word| {
        ["--bin", "--example"].iter().any(|flag| {
            word == flag
                || word
                    .strip_prefix(flag)
                    .is_some_and(|rest| rest.starts_with('='))
        })
    });
    let names_package = words.iter().any(|word| {
        matches!(*word, "-p" | "--package")
            || word.starts_with("--package=")
            || word.starts_with("-p") && word.len() > 2
    });
    let plans_only = building
        && words.iter().any(|word| {
            matches!(*word, "--unit-graph" | "--build-plan" | "--print")
                || word.starts_with("--print=")
        });
    match (program, arguments) {
        _ if plans_only => Some(
            "inputs build does not accept a plan-only Cargo invocation (`--unit-graph`, `--build-plan` or `--print`): it compiles nothing, so no receipt of a compile is current; run it outside inputs build",
        ),
        ("cargo", ["fix", ..]) => Some(
            "inputs build does not run cargo fix: it edits sources after compiling them, so no receipt of its compile is current; run it outside inputs build",
        ),
        _ if running && !(names_target && names_package) => Some(
            "inputs build cannot tell which target cargo run builds; name the package with -p and the target with --bin or --example",
        ),
        _ => None,
    }
}

const RUN_VALUED: [&str; 18] = [
    "-p",
    "--package",
    "--bin",
    "--example",
    "-F",
    "--features",
    "--profile",
    "--target",
    "--manifest-path",
    "--message-format",
    "-j",
    "--jobs",
    "--config",
    "-Z",
    "--color",
    "--target-dir",
    "--lockfile-path",
    "--artifact-dir",
];

fn clustered_valued(word: &str) -> bool {
    word.strip_prefix('-')
        .filter(|flags| !flags.starts_with('-'))
        .is_some_and(|flags| matches!(flags.trim_start_matches(['q', 'v', 'r']), "F" | "p" | "j"))
}

pub(super) fn run_program_arguments(command: &[String], subcommand: usize) -> usize {
    let mut index = subcommand + 2;
    while let Some(word) = command.get(index) {
        if word == "--" || !word.starts_with('-') {
            return index;
        }
        index += if RUN_VALUED.contains(&word.as_str()) || clustered_valued(word) {
            2
        } else {
            1
        };
    }
    command.len()
}

pub(super) fn build_of(command: &[String], subcommand: usize) -> Vec<String> {
    let end = run_program_arguments(command, subcommand);
    let mut build = command[..end.min(command.len())].to_vec();
    "build".clone_into(&mut build[subcommand + 1]);
    build
}
