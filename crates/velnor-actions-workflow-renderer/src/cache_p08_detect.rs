// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn detector_words(run: &[String]) -> Vec<String> {
    if crate::commands::is_inline_shell(run) {
        run[2].split_whitespace().map(unquote_word).collect()
    } else {
        run.iter().map(|arg| unquote_word(arg.as_str())).collect()
    }
}

fn unquote_word(word: &str) -> String {
    word.strip_prefix('\'')
        .and_then(|inner| inner.strip_suffix('\''))
        .or_else(|| {
            word.strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
        })
        .unwrap_or(word)
        .to_owned()
}
