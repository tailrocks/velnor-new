// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use crate::expression::is_identifier;

pub const HELPER_COMMANDS: [&str; 34] = [
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
    "install-gh",
    "stage-bundle",
    "qualification",
];

pub(crate) fn subcommand(run: &str) -> Option<String> {
    shell_words(run).into_iter().nth(1)
}

/// The words of a `run` value from its real command word on.
///
/// The value is split into shell words (quotes honoured). Leading `NAME=VALUE`
/// assignments are skipped, and so is every `env` prefix with its options
/// (`-C <dir>`, `-u <name>`, `-i`, `-S <string>`, `--chdir=<dir>`, `--`, clustered
/// short options such as `-iC <dir>`) and assignments, nested `env` included. A
/// run value is one simple command, which the render rules enforce (no `;`, `&`,
/// `|` or line breaks), so no later command is looked at.
pub(crate) fn command_words(run: &str) -> Vec<String> {
    let mut words: VecDeque<String> = shell_words(run).into();
    loop {
        while words.front().is_some_and(|word| assignment(word)) {
            words.pop_front();
        }
        if !words
            .front()
            .is_some_and(|word| matches!(word.as_str(), "env" | "/usr/bin/env" | "/bin/env"))
        {
            return words.into();
        }
        words.pop_front();
        skip_env_options(&mut words);
    }
}

fn skip_env_options(words: &mut VecDeque<String>) {
    while let Some(word) = words.front().cloned() {
        if assignment(&word) {
            words.pop_front();
        } else if word == "--" {
            words.pop_front();
            return;
        } else if let Some(long) = word.strip_prefix("--") {
            words.pop_front();
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(name, value)| (name, Some(value)));
            match (name, attached) {
                ("split-string", Some(text)) => split_into(words, text),
                ("split-string", None) => split_next(words),
                ("chdir" | "unset" | "argv0", None) => {
                    words.pop_front();
                }
                _ => {}
            }
        } else if word.starts_with('-') {
            words.pop_front();
            skip_short_cluster(words, &word[1..]);
        } else {
            return;
        }
    }
}

fn skip_short_cluster(words: &mut VecDeque<String>, cluster: &str) {
    for (index, flag) in cluster.char_indices() {
        if !matches!(flag, 'C' | 'u' | 'a' | 'P' | 'S') {
            continue;
        }
        let attached = &cluster[index + flag.len_utf8()..];
        match (flag, attached.is_empty()) {
            ('S', false) => split_into(words, attached),
            ('S', true) => split_next(words),
            (_, true) => {
                words.pop_front();
            }
            (_, false) => {}
        }
        return;
    }
}

fn split_next(words: &mut VecDeque<String>) {
    if let Some(text) = words.pop_front() {
        split_into(words, &text);
    }
}

fn split_into(words: &mut VecDeque<String>, text: &str) {
    for word in shell_words(text).into_iter().rev() {
        words.push_front(word);
    }
}

/// Whether the command word of a `run` value is the `velnor-ci` helper.
///
/// The command word is the first word `command_words` finds. It counts when its
/// basename is exactly `velnor-ci`, so `velnor-ci`, `/usr/local/bin/velnor-ci`
/// and `~/bin/velnor-ci` do, and `velnor-cix` and `echo velnor-ci-notes` do not.
/// A path that stays inside the workspace (`./velnor-ci`, `stage/velnor-ci`) is a
/// binary of the repository, not the pinned helper that `helper = true`
/// installs, and the write token rule treats it as repository code.
/// `cargo run -p velnor-ci` is not the helper either.
pub(crate) fn calls_velnor_ci(run: &str) -> bool {
    command_words(run).first().is_some_and(|word| {
        word.rsplit('/').next() == Some("velnor-ci") && !inside_workspace(word)
    })
}

fn inside_workspace(word: &str) -> bool {
    word.contains('/')
        && !word.starts_with(['/', '~'])
        && !word.contains('$')
        && !word.split('/').any(|part| part == "..")
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
