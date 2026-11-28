// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::doc_steps::tokens::Token;

const NAMES: &[&str] = &["concat", "env", "include", "include_str", "include_bytes"];
const STANDARD: &[&str] = &["std", "core", "alloc"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Statement {
    Outside,
    Opening,
    Standard,
    Other,
}

pub(super) fn shadows(tokens: &[Token<'_>]) -> bool {
    let mut statement = Statement::Outside;
    for (at, token) in tokens.iter().enumerate() {
        match (statement, token) {
            (_, Token::Ident("macro_use")) => return true,
            (_, Token::Ident("macro_rules")) => {
                if let [Token::Punct('!'), Token::Ident(name), ..] = &tokens[at + 1..]
                    && NAMES.contains(name)
                {
                    return true;
                }
            }
            (Statement::Outside, Token::Ident(name)) if qualified(tokens, at, name) => {
                return true;
            }
            (Statement::Outside, Token::Ident("use")) => statement = Statement::Opening,
            (_, Token::Punct(';')) => statement = Statement::Outside,
            (Statement::Opening, Token::Ident(root)) if STANDARD.contains(root) => {
                statement = Statement::Standard;
            }
            (Statement::Standard, Token::Ident("as")) if renamed(tokens, at) => return true,
            (Statement::Outside | Statement::Standard, _)
            | (Statement::Opening, Token::Punct(':')) => {}
            (Statement::Opening | Statement::Other, token) => {
                statement = Statement::Other;
                if imports(token, tokens.get(at + 1)) {
                    return true;
                }
            }
        }
    }
    false
}

fn imports(token: &Token<'_>, next: Option<&Token<'_>>) -> bool {
    match token {
        Token::Punct('*') => true,
        Token::Ident(name) => {
            NAMES.contains(name) && matches!(next, Some(Token::Punct(',' | '}' | ';')) | None)
        }
        _ => false,
    }
}

fn renamed(tokens: &[Token<'_>], at: usize) -> bool {
    let item = at.checked_sub(1).and_then(|before| tokens.get(before));
    matches!(
        (item, tokens.get(at + 1)),
        (Some(Token::Ident(item)), Some(Token::Ident(bound)))
            if NAMES.contains(bound) && item != bound
    )
}

fn qualified(tokens: &[Token<'_>], at: usize, name: &str) -> bool {
    if !NAMES.contains(&name) || !matches!(tokens.get(at + 1), Some(Token::Punct('!'))) {
        return false;
    }
    let [.., Token::Punct(':'), Token::Punct(':')] = &tokens[..at] else {
        return false;
    };
    let path = &tokens[..at - 2];
    !matches!(
        path,
        [.., Token::Ident(root)] if STANDARD.contains(root)
            && !matches!(path, [.., Token::Ident(_), Token::Punct(':'), Token::Punct(':'), _])
    )
}
