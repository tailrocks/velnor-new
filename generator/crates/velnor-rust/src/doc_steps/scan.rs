// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tokens::{Token, tokens};

const RUSTDOC_SETTINGS: [&str; 2] = ["rustdoc", "missing_docs"];
const DOC_MARKERS: [&str; 4] = ["///", "//!", "/**", "/*!"];

#[derive(Default)]
pub(crate) struct Found {
    pub(crate) docs: bool,
    pub(crate) fenced: bool,
}

impl Found {
    fn both(&mut self) {
        self.docs = true;
        self.fenced = true;
    }

    pub(crate) fn scan_text(&mut self, text: &str) {
        self.docs |= RUSTDOC_SETTINGS
            .iter()
            .any(|setting| text.contains(setting))
            || text.lines().any(|line| {
                line.trim_start()
                    .strip_prefix("include")
                    .is_some_and(|rest| rest.trim_start().starts_with('='))
            });
    }

    pub(crate) fn scan_rust(&mut self, text: &str, strings_may_hold_docs: bool) {
        self.scan_text(text);
        if strings_may_hold_docs
            && (DOC_MARKERS.iter().any(|marker| text.contains(marker)) || has_word(text, "doc"))
        {
            self.both();
        }
        for token in tokens(text) {
            match token {
                Token::Doc { fenced } => {
                    self.docs = true;
                    self.fenced |= fenced;
                }
                Token::Ident(word) if names_doc_input(word) => self.both(),
                _ => {}
            }
        }
    }
}

fn names_doc_input(word: &str) -> bool {
    word.starts_with("doc") || word.starts_with("include") || word == "path"
}

fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let identifier = |c: char| c.is_alphanumeric() || c == '_';
        !text[..at].chars().next_back().is_some_and(identifier)
            && !text[at + word.len()..]
                .chars()
                .next()
                .is_some_and(identifier)
    })
}
