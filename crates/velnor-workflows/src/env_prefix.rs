// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use crate::command_scan::{Word, segments};
use crate::wrappers::assignment;

/// Skips the options and assignments after `env`. `false` means `-S` held text that is not
/// shell words.
///
/// Options: `-C <dir>`, `-u <name>`, `-i`, `-S <string>`, `--chdir=<dir>`, `--`, and clustered
/// short options such as `-iC <dir>`.
pub(crate) fn skip(words: &mut VecDeque<Word>) -> bool {
    while let Some(word) = words.front().map(|word| word.text.clone()) {
        if assignment(&word) {
            words.pop_front();
        } else if word == "--" {
            words.pop_front();
            return true;
        } else if let Some(long) = word.strip_prefix("--") {
            words.pop_front();
            if !long_option(words, long) {
                return false;
            }
        } else if let Some(cluster) = word.strip_prefix('-') {
            words.pop_front();
            if !short_cluster(words, cluster) {
                return false;
            }
        } else {
            return true;
        }
    }
    true
}

fn long_option(words: &mut VecDeque<Word>, long: &str) -> bool {
    let (name, attached) = long
        .split_once('=')
        .map_or((long, None), |(name, value)| (name, Some(value)));
    match (name, attached) {
        ("split-string", Some(text)) => split_into(words, text),
        ("split-string", None) => split_next(words),
        ("chdir" | "unset" | "argv0", None) => {
            words.pop_front();
            true
        }
        _ => true,
    }
}

fn short_cluster(words: &mut VecDeque<Word>, cluster: &str) -> bool {
    for (index, flag) in cluster.char_indices() {
        if !matches!(flag, 'C' | 'u' | 'a' | 'P' | 'S') {
            continue;
        }
        let attached = &cluster[index + flag.len_utf8()..];
        return match (flag, attached.is_empty()) {
            ('S', false) => split_into(words, attached),
            ('S', true) => split_next(words),
            (_, true) => {
                words.pop_front();
                true
            }
            (_, false) => true,
        };
    }
    true
}

fn split_next(words: &mut VecDeque<Word>) -> bool {
    words
        .pop_front()
        .is_none_or(|word| split_into(words, &word.text))
}

fn split_into(words: &mut VecDeque<Word>, text: &str) -> bool {
    let Some(parts) = segments(text) else {
        return false;
    };
    for word in parts.into_iter().flatten().rev() {
        words.push_front(word);
    }
    true
}
