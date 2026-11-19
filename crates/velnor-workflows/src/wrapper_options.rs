// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use crate::command_scan::Word;
use crate::wrappers::Next;

/// The options of a command wrapper. A word that is not listed is not skipped as an option:
/// `skip` returns `false` for it, and the caller treats the command as unreadable.
pub(crate) struct Options {
    /// Short options without a value, such as `-f`.
    flags: &'static str,
    /// Short options with a value, attached (`-n5`) or in the next word (`-n 5`).
    with_value: &'static str,
    /// Short options whose value, if any, is attached (`-i`, `-iREPL`).
    optional_value: &'static str,
    /// Long options without a value (`--foreground`; a `=value` is accepted).
    long: &'static [&'static str],
    /// Long options with a value (`--signal=KILL` or `--signal KILL`).
    long_with_value: &'static [&'static str],
}

const NONE: Options = Options {
    flags: "",
    with_value: "",
    optional_value: "",
    long: &[],
    long_with_value: &[],
};

pub(crate) const NOHUP: Options = NONE;
pub(crate) const EXEC: Options = Options {
    flags: "cl",
    with_value: "a",
    ..NONE
};
pub(crate) const NICE: Options = Options {
    flags: "0123456789",
    with_value: "n",
    long_with_value: &["adjustment"],
    ..NONE
};
pub(crate) const IONICE: Options = Options {
    flags: "t",
    with_value: "cn",
    long: &["ignore"],
    long_with_value: &["class", "classdata"],
    ..NONE
};
pub(crate) const STDBUF: Options = Options {
    with_value: "ioe",
    long_with_value: &["input", "output", "error"],
    ..NONE
};
pub(crate) const TIME: Options = Options {
    flags: "pvalh",
    with_value: "fo",
    long: &["portability", "verbose", "append"],
    long_with_value: &["format", "output"],
    ..NONE
};
pub(crate) const TIMEOUT: Options = Options {
    flags: "fpv",
    with_value: "sk",
    long: &["foreground", "preserve-status", "verbose"],
    long_with_value: &["signal", "kill-after"],
    ..NONE
};
pub(crate) const XARGS: Options = Options {
    flags: "0prtxo",
    with_value: "aEIJLnPRsSd",
    optional_value: "ile",
    long: &[
        "null",
        "verbose",
        "no-run-if-empty",
        "open-tty",
        "interactive",
        "exit",
        "show-limits",
        "replace",
        "eof",
        "max-lines",
    ],
    long_with_value: &[
        "arg-file",
        "delimiter",
        "max-args",
        "max-chars",
        "max-procs",
        "process-slot-var",
    ],
};

impl Options {
    /// Skips the options at the front of `words`, and a `--` that ends them.
    pub(crate) fn skip(&self, words: &mut VecDeque<Word>) -> bool {
        while let Some(text) = words.front().map(|word| word.text.clone()) {
            if text == "--" {
                words.pop_front();
                return true;
            }
            if let Some(long) = text.strip_prefix("--") {
                words.pop_front();
                if !self.long_option(words, long) {
                    return false;
                }
            } else if let Some(cluster) = text.strip_prefix('-').filter(|rest| !rest.is_empty()) {
                words.pop_front();
                if !self.cluster(words, cluster) {
                    return false;
                }
            } else {
                return true;
            }
        }
        true
    }

    fn long_option(&self, words: &mut VecDeque<Word>, long: &str) -> bool {
        let (name, attached) = long
            .split_once('=')
            .map_or((long, false), |(name, _)| (name, true));
        if self.long.contains(&name) {
            true
        } else if self.long_with_value.contains(&name) {
            attached || words.pop_front().is_some()
        } else {
            false
        }
    }

    fn cluster(&self, words: &mut VecDeque<Word>, cluster: &str) -> bool {
        for (index, flag) in cluster.char_indices() {
            if self.flags.contains(flag) {
                continue;
            }
            if self.optional_value.contains(flag) {
                return true;
            }
            let attached = index + flag.len_utf8() < cluster.len();
            return self.with_value.contains(flag) && (attached || words.pop_front().is_some());
        }
        true
    }
}

/// `command [-p] name ...` runs `name`; `command -v name` and `-V` only look it up.
pub(crate) fn command_query(words: &mut VecDeque<Word>) -> Next {
    while let Some(text) = words.front().map(|word| word.text.clone()) {
        let Some(cluster) = text.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            break;
        };
        words.pop_front();
        if cluster == "-" {
            break;
        }
        if cluster.contains(['v', 'V']) {
            return Next::Nothing;
        }
        if cluster.chars().any(|flag| flag != 'p') {
            return Next::Opaque;
        }
    }
    Next::Continue
}

/// `sh`, `bash`, ... : `-c SCRIPT` runs the script, which is read as a `run` value when it is
/// plain text; without `-c` the shell runs a script file.
pub(crate) fn shell(words: &mut VecDeque<Word>) -> Next {
    let mut script_follows = false;
    while let Some(text) = words.front().map(|word| word.text.clone()) {
        if text == "--" {
            words.pop_front();
            break;
        }
        if let Some(long) = text.strip_prefix("--") {
            words.pop_front();
            match long {
                "norc" | "noprofile" | "posix" | "login" | "restricted" | "verbose"
                | "noediting" => {}
                "rcfile" | "init-file" => {
                    words.pop_front();
                }
                _ => return Next::Opaque,
            }
        } else if let Some(cluster) = text
            .strip_prefix(['-', '+'])
            .filter(|rest| !rest.is_empty())
        {
            words.pop_front();
            match shell_cluster(words, cluster) {
                Some(has_script) => script_follows |= has_script,
                None => return Next::Opaque,
            }
        } else {
            break;
        }
    }
    if !script_follows {
        return Next::Plain;
    }
    match words.pop_front() {
        Some(script) if !script.dynamic && !script.text.contains(['$', '`']) => {
            Next::Script(script.text)
        }
        _ => Next::Opaque,
    }
}

/// Whether a cluster of shell options holds `c`; `None` for an option the scan does not know.
fn shell_cluster(words: &mut VecDeque<Word>, cluster: &str) -> Option<bool> {
    let mut script = false;
    for flag in cluster.chars() {
        match flag {
            'c' => script = true,
            'o' | 'O' => {
                words.pop_front();
                return Some(script);
            }
            flag if "ebfhiklmnprtuvxBCEHPT".contains(flag) => {}
            _ => return None,
        }
    }
    Some(script)
}
