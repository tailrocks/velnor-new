// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_workflows::{Invoked, invoked};

use crate::rules::Rule;

const ISOLATION: [&str; 3] = ["--no-config", "--no-env", "--no-hooks"];

/// The rules a `run` value breaks in a job that holds the release App token: `mise` runs only
/// with `--no-config --no-env --no-hooks`, so it never reads the repository's config.
///
/// `velnor_workflows::invoked` looks through assignments, `env`, `nohup`, `exec`, `command`,
/// `nice`, `timeout`, `stdbuf`, `time`, `xargs` and `sh -c` scripts, and reads every command of
/// a sequence. A command it cannot read (`eval`, `source`, `sudo`, an unknown wrapper option,
/// a command word that is an expansion) may run anything, `mise` included, so it fails closed.
pub(crate) fn rules(run: &str) -> Vec<Rule> {
    if run.starts_with(crate::helper_integrity::INVOCATION_PREFIX) {
        return Vec::new();
    }
    let mut rules = Vec::new();
    for call in invoked(run) {
        let rule = match call {
            Invoked::Command(words) => (names_mise(&words[0]) && !isolated(&words[1..]))
                .then_some(Rule::MiseReadsRepositoryConfig),
            Invoked::Opaque(words) => opaque(&words),
        };
        if let Some(rule) = rule
            && !rules.contains(&rule)
        {
            rules.push(rule);
        }
    }
    rules
}

/// `words` holds each word followed by the pieces of its text, so a word that is one piece
/// appears twice in a row.
fn opaque(words: &[String]) -> Option<Rule> {
    let mut words = words.to_vec();
    words.dedup();
    if words.iter().skip(1).any(|word| names_mise(word)) {
        return Some(Rule::MiseReadsRepositoryConfig);
    }
    match words.first() {
        Some(first) if names_mise(first) => {
            (!isolated(&words[1..])).then_some(Rule::MiseReadsRepositoryConfig)
        }
        Some(first) if named_through_expansion(first) => None,
        _ => Some(Rule::UnreadableCommandInTokenJob),
    }
}

fn names_mise(word: &str) -> bool {
    word.rsplit('/').next() == Some("mise")
}

fn isolated(arguments: &[String]) -> bool {
    let before_command = arguments.iter().take_while(|word| *word != "--");
    let flags: Vec<&String> = before_command.collect();
    ISOLATION
        .iter()
        .all(|flag| flags.iter().any(|word| *word == flag))
}

/// `$RUNNER_TEMP/release-plz`: an expansion in the directory part and a plain file name, so the
/// program is known by its name even though the directory is not.
fn named_through_expansion(word: &str) -> bool {
    let Some((directory, name)) = word.rsplit_once('/') else {
        return false;
    };
    directory.contains('$')
        && directory
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_$-{}./".contains(c))
        && !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}
