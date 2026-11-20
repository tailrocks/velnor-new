// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A shell text split into simple commands, each as its words.

/// The simple commands of a shell text, each as its words, quotes removed. A command starts
/// after a line break, `;`, `&`, `|`, `(` or `)`. A `#` outside a word starts a comment.
pub(super) fn segments(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut segment = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut comment = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if comment {
            if c != '\n' {
                continue;
            }
            comment = false;
        }
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else if c == '\\' && open == '"' {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                in_word = true;
            }
            '\\' => {
                if let Some(next) = chars.next().filter(|next| *next != '\n') {
                    word.push(next);
                    in_word = true;
                }
            }
            '#' if !in_word => comment = true,
            '\n' | ';' | '&' | '|' | '(' | ')' => {
                end_word(&mut word, &mut in_word, &mut segment);
                if !segment.is_empty() {
                    out.push(std::mem::take(&mut segment));
                }
            }
            c if c.is_whitespace() => end_word(&mut word, &mut in_word, &mut segment),
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut segment);
    if !segment.is_empty() {
        out.push(segment);
    }
    out
}

fn end_word(word: &mut String, in_word: &mut bool, segment: &mut Vec<String>) {
    if *in_word {
        segment.push(std::mem::take(word));
        *in_word = false;
    }
}
