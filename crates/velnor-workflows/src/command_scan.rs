// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::str::Chars;

/// One shell word with its quotes removed.
#[derive(Clone, Debug, Default)]
pub(crate) struct Word {
    pub(crate) text: String,
    /// An unquoted or double-quoted `$` or backtick: the text is not what the shell runs.
    pub(crate) dynamic: bool,
    redirect: bool,
}

/// The simple commands of a shell text, each as its words.
///
/// A new command starts at a line break, `;`, `&`, `|` (so also `&&` and `||`), `(` and `)`.
/// Quotes and backslashes are honoured; a redirection (`>file`, `2>&1`, `< in`) is dropped
/// with its target. `None` means the text cannot be read as shell words (an open quote).
pub(crate) fn segments(text: &str) -> Option<Vec<Vec<Word>>> {
    let mut lexer = Lexer::default();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        lexer.feed(c, &mut chars);
    }
    if lexer.quote.is_some() {
        return None;
    }
    lexer.end_segment();
    Some(lexer.segments)
}

#[derive(Default)]
struct Lexer {
    segments: Vec<Vec<Word>>,
    segment: Vec<Word>,
    word: Word,
    started: bool,
    quote: Option<char>,
}

impl Lexer {
    fn push(&mut self, c: char) {
        self.word.text.push(c);
        self.started = true;
    }

    fn push_dynamic(&mut self, c: char) {
        self.word.dynamic = true;
        self.push(c);
    }

    fn end_word(&mut self) {
        if self.started {
            self.segment.push(std::mem::take(&mut self.word));
            self.started = false;
        }
    }

    fn end_segment(&mut self) {
        self.end_word();
        let words = without_redirections(std::mem::take(&mut self.segment));
        if !words.is_empty() {
            self.segments.push(words);
        }
    }

    fn file_descriptor(&self) -> bool {
        !self.word.text.is_empty() && self.word.text.chars().all(|c| c.is_ascii_digit())
    }

    fn feed(&mut self, c: char, rest: &mut Chars<'_>) {
        match (self.quote, c) {
            (Some(open), c) if c == open => self.quote = None,
            (Some('"') | None, '\\') => self.escape(rest),
            (Some('"') | None, '$' | '`') => self.push_dynamic(c),
            (None, '\'' | '"') => {
                self.quote = Some(c);
                self.started = true;
            }
            (None, '\n' | ';' | '(' | ')') => self.end_segment(),
            (None, '|' | '&') if !self.in_redirection() => self.end_segment(),
            (None, '<' | '>') => {
                if self.started && !self.word.redirect && !self.file_descriptor() {
                    self.end_word();
                }
                self.word.redirect = true;
                self.push(c);
            }
            (None, c) if c.is_whitespace() => self.end_word(),
            (_, c) => self.push(c),
        }
    }

    /// `>&2`, `<&3` and `>|file` keep their `&` or `|` inside the redirection word.
    fn in_redirection(&self) -> bool {
        self.started && self.word.redirect && self.word.text.ends_with(['<', '>'])
    }

    fn escape(&mut self, rest: &mut Chars<'_>) {
        if let Some(next) = rest.next()
            && next != '\n'
        {
            self.push(next);
        }
    }
}

fn without_redirections(words: Vec<Word>) -> Vec<Word> {
    let mut kept = Vec::new();
    let mut skip_target = false;
    for word in words {
        if std::mem::take(&mut skip_target) {
            continue;
        }
        if word.redirect {
            let operator = word.text.trim_start_matches(|c: char| c.is_ascii_digit());
            skip_target = operator.trim_start_matches(['<', '>', '&', '|']).is_empty();
        } else {
            kept.push(word);
        }
    }
    kept
}

/// The pieces of a text split at whitespace, quotes and shell operators, with no parsing.
pub(crate) fn loose_words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || "'\"`()<>;&|".contains(c))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}
