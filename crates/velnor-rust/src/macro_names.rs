// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;

use crate::doc_steps::tokens::Token;

const SHADOWABLE: &[&str] = &["env", "concat", "include_str", "include_bytes", "include"];
const BUNDLED: &[&str] = &["std", "core", "alloc"];

pub(crate) fn shadows(tokens: &[Token<'_>]) -> bool {
    let mut in_use = false;
    tokens.iter().enumerate().any(|(at, token)| match token {
        Token::Ident("use") => {
            in_use = true;
            false
        }
        Token::Punct(';') => {
            in_use = false;
            false
        }
        Token::Ident("macro_rules") => matches!(
            tokens[at + 1..],
            [Token::Punct('!'), Token::Ident(name), ..] if SHADOWABLE.contains(&name)
        ),
        Token::Ident("macro") => matches!(
            tokens.get(at + 1),
            Some(Token::Ident(name)) if SHADOWABLE.contains(name)
        ),
        Token::Ident("extern") => {
            matches!(
                tokens[at + 1..],
                [Token::Ident("crate"), Token::Ident(name), ..] if !BUNDLED.contains(&name)
            ) && macro_use_attribute(&tokens[..at])
                || matches!(
                    tokens[at + 1..],
                    [Token::Ident("crate"), Token::Ident(name), Token::Ident("as"), Token::Ident(alias), ..]
                        if BUNDLED.contains(&alias) && name != alias
                )
        }
        Token::Ident(name) if in_use && SHADOWABLE.contains(name) => {
            !from_std(&tokens[..at])
                && !matches!(
                    tokens[at + 1..],
                    [Token::Punct(':'), Token::Punct(':'), ..] | [Token::Ident("as"), ..]
                )
                && invoked(tokens, name)
        }
        _ => false,
    })
}

fn macro_use_attribute(before: &[Token<'_>]) -> bool {
    let mut end = before.len();
    while before[..end].last() == Some(&Token::Punct(']')) {
        let Some(open) = before[..end]
            .iter()
            .rposition(|token| *token == Token::Punct('['))
        else {
            return false;
        };
        if before[open..end].contains(&Token::Ident("macro_use")) {
            return true;
        }
        end = open.saturating_sub(1);
    }
    false
}

pub(crate) fn names_builtin(before: &[Token<'_>]) -> bool {
    match before {
        [
            head @ ..,
            Token::Ident("std" | "core"),
            Token::Punct(':'),
            Token::Punct(':'),
        ] => !matches!(
            head,
            [.., Token::Ident(_), Token::Punct(':'), Token::Punct(':')]
        ),
        [.., Token::Punct(':'), Token::Punct(':')] => false,
        _ => true,
    }
}

fn invoked(tokens: &[Token<'_>], name: &str) -> bool {
    tokens.windows(3).any(|window| {
        matches!(
            window,
            [Token::Ident(found), Token::Punct('!'), Token::Punct('(' | '[' | '{')] if *found == name
        )
    })
}

fn from_std(before: &[Token<'_>]) -> bool {
    matches!(
        before,
        [
            ..,
            Token::Ident("std" | "core"),
            Token::Punct(':'),
            Token::Punct(':')
        ]
    )
}

pub(crate) fn macro_bodies(tokens: &[Token<'_>]) -> Vec<Range<usize>> {
    let mut bodies = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let (open, has_arguments) = match (token, &tokens[at + 1..]) {
            (Token::Ident("macro_rules"), [Token::Punct('!'), Token::Ident(_), ..]) => {
                (at + 3, false)
            }
            (Token::Ident("macro"), [Token::Ident(_), ..]) => (at + 2, true),
            _ => continue,
        };
        let mut end = group_end(tokens, open);
        if has_arguments && tokens.get(open) == Some(&Token::Punct('(')) {
            end = group_end(tokens, end);
        }
        bodies.push(at..end);
    }
    bodies
}

pub(crate) fn group_end(tokens: &[Token<'_>], open: usize) -> usize {
    let mut depth = 0_usize;
    for (at, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}
