// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tokens::{Token, tokens};

const RUSTDOC_SETTINGS: [&str; 2] = ["rustdoc", "missing_docs"];
const DOC_MARKERS: [&str; 4] = ["///", "//!", "/**", "/*!"];
const BINDINGS: [&str; 4] = ["let", "mut", "ref", "const"];

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
        let tokens = tokens(text);
        let mut depth = 0_usize;
        for (at, token) in tokens.iter().enumerate() {
            match token {
                Token::Doc { fenced } => {
                    self.docs = true;
                    self.fenced |= fenced;
                }
                Token::Punct('[') if depth > 0 || opens_attribute(&tokens, at) => depth += 1,
                Token::Punct(']') => depth = depth.saturating_sub(1),
                Token::Ident("include") if tokens.get(at + 1) == Some(&Token::Punct('!')) => {
                    self.both();
                }
                Token::Ident("path") if depth > 0 && assigns(&tokens, at) => self.both(),
                Token::Ident("doc") if assigns(&tokens, at) && !binds(&tokens, at) => self.both(),
                Token::Ident("cfg" | "cfg_attr") => self.docs |= names_doc(&tokens, at),
                _ => {}
            }
        }
    }
}

fn opens_attribute(tokens: &[Token<'_>], at: usize) -> bool {
    let before = |back: usize| at.checked_sub(back).and_then(|i| tokens.get(i));
    before(1) == Some(&Token::Punct('#'))
        || (before(1) == Some(&Token::Punct('!')) && before(2) == Some(&Token::Punct('#')))
}

fn assigns(tokens: &[Token<'_>], at: usize) -> bool {
    tokens.get(at + 1) == Some(&Token::Punct('='))
        && !matches!(tokens.get(at + 2), Some(Token::Punct('=' | '>')))
}

fn binds(tokens: &[Token<'_>], at: usize) -> bool {
    match at.checked_sub(1).and_then(|before| tokens.get(before)) {
        Some(Token::Ident(word)) => BINDINGS.contains(word),
        Some(Token::Punct('.')) => true,
        _ => false,
    }
}

fn names_doc(tokens: &[Token<'_>], at: usize) -> bool {
    let open = at + 1 + usize::from(tokens.get(at + 1) == Some(&Token::Punct('!')));
    if tokens.get(open) != Some(&Token::Punct('(')) {
        return false;
    }
    let mut depth = 0_usize;
    for token in &tokens[open..] {
        match token {
            Token::Punct('(') => depth += 1,
            Token::Punct(')') => {
                depth -= 1;
                if depth == 0 {
                    return false;
                }
            }
            Token::Ident(word) if word.starts_with("doc") => return true,
            _ => {}
        }
    }
    false
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
