// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::markdown::tested_text;
use super::tokens::{Token, tokens};

const RUSTDOC_SETTINGS: [&str; 3] = ["rustdoc", "missing_docs", "missing_crate_level_docs"];

#[derive(Default)]
pub(crate) struct Reached {
    pub(crate) includes: Vec<String>,
    pub(crate) modules: Vec<String>,
}

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

    pub(crate) fn scan_rust(&mut self, text: &str, counts: bool) -> Reached {
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
        let mut reached = Reached::default();
        for (index, token) in tokens.iter().enumerate() {
            match *token {
                Token::Doc { fenced, tested } => {
                    self.docs = true;
                    self.fenced |= fenced;
                    self.written |= tested && counts;
                }
                Token::Ident(word) if names_doc_input(word) => {
                    self.both();
                    match &tokens[index + 1..] {
                        [Token::Punct('='), Token::Str(literal), ..]
                            if attribute(&tokens, index) && has_test(literal) =>
                        {
                            self.written |= counts;
                        }
                        [
                            Token::Punct('='),
                            Token::Ident("include_str"),
                            Token::Punct('!'),
                            Token::Punct('(' | '[' | '{'),
                            Token::Str(path),
                            ..,
                        ] if attribute(&tokens, index) && counts => {
                            reached.includes.push((*path).to_owned());
                        }
                        [Token::Punct('='), Token::Str(path), Token::Punct(']'), ..]
                            if word == "path"
                                && counts
                                && matches!(
                                    &tokens[..index],
                                    [.., Token::Punct('#'), Token::Punct('[')]
                                ) =>
                        {
                            reached.modules.push((*path).to_owned());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        reached
    }

    pub(crate) fn scan_included(&mut self, text: &str) {
        self.written |= tested_text(text);
    }
}

fn has_test(literal: &str) -> bool {
    tested_text(&literal.replace("\\n", "\n"))
}

fn attribute(tokens: &[Token<'_>], index: usize) -> bool {
    if tokens[index] != Token::Ident("doc") {
        return false;
    }
    let mut depth = 0_usize;
    for at in (0..index).rev() {
        match tokens[at] {
            Token::Punct(')' | ']') => depth += 1,
            Token::Punct('(') if depth == 0 => {
                return at > 0 && tokens[at - 1] == Token::Ident("cfg_attr");
            }
            Token::Punct('[') if depth == 0 => {
                return matches!(
                    &tokens[..at],
                    [.., Token::Punct('#')] | [.., Token::Punct('#'), Token::Punct('!')]
                );
            }
            Token::Punct('(' | '[') => depth -= 1,
            Token::Punct(';' | '{' | '}') if depth == 0 => return false,
            _ => {}
        }
    }
    false
}

fn names_doc_input(word: &str) -> bool {
    word.starts_with("doc") || word.starts_with("include") || word == "path"
}
