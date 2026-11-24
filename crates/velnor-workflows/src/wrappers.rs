// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use crate::command_scan::{Word, loose_words, segments};
use crate::env_prefix;
use crate::expression::is_identifier;
use crate::wrapper_options::{
    EXEC, IONICE, NICE, NOHUP, STDBUF, TIME, TIMEOUT, XARGS, command_query, shell,
};

/// A command a `run` value starts, after every wrapper is looked through.
#[derive(Debug)]
pub enum Invoked {
    /// The command and its arguments; the first word is the command word.
    Command(Vec<String>),
    /// A command the scan cannot read (`eval`, `sudo`, a shell script with an expansion, a
    /// command word that is an expansion, ...). It counts as repository code. The words are
    /// every piece of text it holds, so a `velnor-ci` inside it is still seen.
    Opaque(Vec<String>),
}

/// How many shell scripts may nest (`sh -c "sh -c '...'"`) before the scan gives up.
const MAX_DEPTH: usize = 8;
/// How many wrappers may precede one command before the scan gives up.
const MAX_CHAIN: usize = 32;

/// Words that run what follows them, or that the scan does not follow into.
const OPAQUE: &[&str] = &[
    "eval",
    "source",
    ".",
    "sudo",
    "doas",
    "su",
    "setsid",
    "chroot",
    "nsenter",
    "unshare",
    "flock",
    "taskset",
    "chrt",
    "parallel",
    "watch",
    "busybox",
    "caffeinate",
    "trap",
    "script",
    "coproc",
    "unbuffer",
    "fakeroot",
    "numactl",
    "strace",
    "ltrace",
    "valgrind",
    "runuser",
    "pkexec",
    "systemd-run",
    "faketime",
    "chronic",
    "cpulimit",
    "bwrap",
    "firejail",
];
const RESERVED: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "select", "{", "}",
];

/// What the commands of a `run` value really run.
///
/// Every simple command of a sequence (`;`, `&&`, `||`, `|`, `&`, line breaks, subshells)
/// counts. A leading `NAME=VALUE`, `!`, `env` (with its options), `nohup`, `exec`, `builtin`,
/// `command` (a `-v` or `-V` query runs nothing), `nice`, `ionice`, `timeout`, `stdbuf`, `time`
/// and `xargs` (with their options) are looked through, chains of them too. `sh`, `bash`,
/// `dash`, `zsh`, `ksh` and `ash` with `-c` run their script, which is read the same way when
/// it holds no `$` or backtick. Everything the scan cannot follow is `Opaque`, so a wrapper
/// that is not known, an option that is not known and an unreadable script fail closed.
#[must_use]
pub fn invoked(run: &str) -> Vec<Invoked> {
    let mut found = Vec::new();
    scan(run, 0, &mut found);
    found
}

pub(crate) fn assignment(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(name, _)| is_identifier(name, false))
}

pub(crate) enum Next {
    /// The wrapper's words are skipped; the command follows.
    Continue,
    /// An ordinary command.
    Plain,
    /// Nothing runs.
    Nothing,
    Opaque,
    /// A shell script string to read as a `run` value.
    Script(String),
}

fn scan(text: &str, depth: usize, found: &mut Vec<Invoked>) {
    match segments(text) {
        Some(commands) => {
            for words in commands {
                unwrap(words.into(), depth, found);
            }
        }
        None => found.push(Invoked::Opaque(loose_words(text))),
    }
}

fn unwrap(mut words: VecDeque<Word>, depth: usize, found: &mut Vec<Invoked>) {
    let mut chain = 0;
    loop {
        while words.front().is_some_and(|word| assignment(&word.text)) {
            words.pop_front();
        }
        let Some(first) = words.front() else {
            return;
        };
        let snapshot: Vec<String> = words.iter().map(|word| word.text.clone()).collect();
        let next = if depth > MAX_DEPTH || chain > MAX_CHAIN || first.dynamic {
            Next::Opaque
        } else {
            step(&mut words)
        };
        match next {
            Next::Continue => chain += 1,
            Next::Nothing => return,
            Next::Plain => {
                found.push(Invoked::Command(snapshot));
                return;
            }
            Next::Opaque => {
                let words = snapshot
                    .iter()
                    .flat_map(|word| std::iter::once(word.clone()).chain(loose_words(word)))
                    .collect();
                found.push(Invoked::Opaque(words));
                return;
            }
            Next::Script(script) => {
                scan(&script, depth + 1, found);
                return;
            }
        }
    }
}

/// The name a command word runs as: a bare word, or the basename of an absolute path. A
/// relative path is a program of the repository, never a wrapper.
fn command_name(text: &str) -> &str {
    if text.starts_with('/') || !text.contains('/') {
        text.rsplit('/').next().unwrap_or(text)
    } else {
        ""
    }
}

fn skipped(ok: bool) -> Next {
    if ok { Next::Continue } else { Next::Opaque }
}

fn step(words: &mut VecDeque<Word>) -> Next {
    let Some(first) = words.pop_front() else {
        return Next::Nothing;
    };
    let name = command_name(&first.text);
    if OPAQUE.contains(&name) || RESERVED.contains(&name) {
        return Next::Opaque;
    }
    match name {
        "env" => skipped(env_prefix::skip(words)),
        "builtin" | "!" => Next::Continue,
        "command" => command_query(words),
        "nohup" => skipped(NOHUP.skip(words)),
        "exec" => skipped(EXEC.skip(words)),
        "nice" => skipped(NICE.skip(words)),
        "ionice" => skipped(IONICE.skip(words)),
        "stdbuf" => skipped(STDBUF.skip(words)),
        "time" => skipped(TIME.skip(words)),
        "timeout" => timeout(words),
        "xargs" => xargs(words),
        "sh" | "bash" | "dash" | "zsh" | "ksh" | "ash" => shell(words),
        "find" if words.iter().any(|word| is_find_exec(&word.text)) => Next::Opaque,
        _ => Next::Plain,
    }
}

fn is_find_exec(word: &str) -> bool {
    matches!(word, "-exec" | "-execdir" | "-ok" | "-okdir")
}

/// `timeout [options] DURATION command`: the duration is not the command.
fn timeout(words: &mut VecDeque<Word>) -> Next {
    if !TIMEOUT.skip(words) {
        return Next::Opaque;
    }
    match words.pop_front() {
        Some(_) => Next::Continue,
        None => Next::Nothing,
    }
}

/// `xargs [options] command`: the command word is the first word after the options. With a
/// replacement string (`-I {}`, `-i`) the command word can itself be the replaced text.
fn xargs(words: &mut VecDeque<Word>) -> Next {
    let before: Vec<String> = words.iter().map(|word| word.text.clone()).collect();
    if !XARGS.skip(words) {
        return Next::Opaque;
    }
    let consumed = &before[..before.len() - words.len()];
    let replaces = consumed.iter().any(|word| {
        ["-I", "-i", "-J", "-R", "--replace"]
            .iter()
            .any(|option| word.starts_with(option))
    });
    let is_replaced =
        |command: &str| command == "{}" || consumed.iter().any(|w| w.ends_with(command));
    match words.front() {
        None => Next::Nothing,
        Some(command) if replaces && is_replaced(&command.text) => Next::Opaque,
        Some(_) => Next::Continue,
    }
}
