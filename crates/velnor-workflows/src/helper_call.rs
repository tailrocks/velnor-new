// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expression::is_identifier;

pub const HELPER_COMMANDS: [&str; 32] = [
    "report",
    "bench",
    "inputs",
    "release-gate",
    "gate",
    "fetch-reports",
    "check-tag",
    "release",
    "plan",
    "merge-group-passed",
    "write-request",
    "resolve-qualification",
    "prepare",
    "crates-mirror",
    "fetch",
    "private-token-allowed",
    "mbx-store",
    "mbx-seed",
    "tools-local",
    "check-metadata",
    "check-configurations",
    "lane-guard",
    "test-groups",
    "restored",
    "delta",
    "image",
    "cache",
    "run",
    "sources",
    "timings",
    "validate-artifact-id",
    "qualification",
];

pub(crate) fn subcommand(run: &str) -> Option<String> {
    shell_words(run).into_iter().nth(1)
}

/// Whether the command word of a `run` value is the `velnor-ci` helper.
///
/// The value is split into shell words (quotes honoured), then leading
/// `NAME=VALUE` assignments and an `env` prefix with its options and
/// assignments are skipped. The command word counts when its basename is
/// exactly `velnor-ci`, so `./velnor-ci` and `target/release/velnor-ci` do and
/// `velnor-cix` and `echo velnor-ci-notes` do not. `cargo run -p velnor-ci`
/// does not either: a helper built and run in-tree is not the pinned one that
/// `helper = true` installs. A run value is one simple command, which the
/// render rules enforce (no `;`, `&`, `|` or line breaks), so no later command
/// is looked at.
pub(crate) fn calls_velnor_ci(run: &str) -> bool {
    let words = shell_words(run);
    let mut rest = words
        .iter()
        .map(String::as_str)
        .skip_while(|w| assignment(w));
    let mut word = rest.next();
    if word == Some("env") {
        word = rest.find(|w| !assignment(w) && !w.starts_with('-'));
    }
    word.and_then(|word| word.rsplit('/').next()) == Some("velnor-ci")
}

fn assignment(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(name, _)| is_identifier(name, false))
}

fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some('"'), '\\') => word.extend(chars.next()),
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                word.extend(chars.next());
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}
