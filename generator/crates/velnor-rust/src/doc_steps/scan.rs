// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tokens::{Token, tokens};

const RUSTDOC_SETTINGS: [&str; 3] = ["rustdoc", "missing_docs", "missing_crate_level_docs"];

#[derive(Default)]
pub(crate) struct Found {
    pub(crate) docs: bool,
    pub(crate) fenced: bool,
    pub(crate) written: bool,
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

    pub(crate) fn scan_rust(&mut self, text: &str, counts: bool) {
        self.scan_text(text);
        let tokens = tokens(text);
        if tokens.windows(3).any(|window| {
            matches!(
                window,
                [Token::Punct('$'), Token::Punct('{'), Token::Ident("concat")]
            )
        }) {
            self.both();
        }
        for token in tokens {
            match token {
                Token::Doc { fenced } => {
                    self.docs = true;
                    self.fenced |= fenced;
                    self.written |= fenced && counts;
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
