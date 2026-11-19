// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::command_scan::segments;
use crate::wrappers::{Invoked, invoked};

pub const HELPER_COMMANDS: [&str; 33] = [
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
    "merge-queue-check",
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
    let command = segments(run)?.into_iter().next()?;
    command.into_iter().nth(1).map(|word| word.text)
}

/// Whether a command of a `run` value is the `velnor-ci` helper.
///
/// `wrappers::invoked` looks through assignments, `env` and the other command wrappers and
/// reads every command of a sequence, so `nohup velnor-ci check`, `timeout 5 velnor-ci check`
/// and `sh -c 'velnor-ci check'` call the helper like `velnor-ci check`. A command that the
/// scan cannot read counts when any word of it names the helper. A command word counts when
/// its basename is exactly `velnor-ci`, so `velnor-ci`, `/usr/local/bin/velnor-ci` and
/// `~/bin/velnor-ci` do, and `velnor-cix` and `echo velnor-ci-notes` do not. A path that
/// stays inside the workspace (`./velnor-ci`, `stage/velnor-ci`) is a binary of the
/// repository, not the pinned helper that `helper = true` installs, and the write token rule
/// treats it as repository code. `cargo run -p velnor-ci` is not the helper either.
pub(crate) fn calls_velnor_ci(run: &str) -> bool {
    invoked(run).iter().any(|call| match call {
        Invoked::Command(words) => words.first().is_some_and(|word| names_helper(word)),
        Invoked::Opaque(words) => words.iter().any(|word| names_helper(word)),
    })
}

fn names_helper(word: &str) -> bool {
    word.rsplit('/').next() == Some("velnor-ci") && !inside_workspace(word)
}

fn inside_workspace(word: &str) -> bool {
    word.contains('/')
        && !word.starts_with(['/', '~'])
        && !word.contains('$')
        && !word.split('/').any(|part| part == "..")
}
