// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::doc_steps::tokens::Token;

pub(crate) fn path_attributes<'t>(tokens: &[Token<'t>]) -> Vec<Option<&'t str>> {
    let mut found = Vec::new();
    let mut depth = 0_usize;
    let mut at = 0;
    while at < tokens.len() {
        match tokens[at] {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') => depth = depth.saturating_sub(1),
            Token::Punct('#') => {
                let open = match tokens[at + 1..] {
                    [Token::Punct('!'), Token::Punct('['), ..] => at + 2,
                    [Token::Punct('['), ..] => at + 1,
                    _ => {
                        at += 1;
                        continue;
                    }
                };
                let end = bracket_end(tokens, open);
                found.extend(
                    tokens[open..end]
                        .windows(3)
                        .filter(|window| {
                            matches!(window, [Token::Ident("path"), Token::Punct('='), _])
                        })
                        .map(|window| match window[2] {
                            Token::Str(literal) if depth == 0 => Some(literal),
                            _ => None,
                        }),
                );
                at = end;
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    found
}

fn bracket_end(tokens: &[Token<'_>], open: usize) -> usize {
    let mut depth = 0_usize;
    for (at, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Punct('[') => depth += 1,
            Token::Punct(']') => {
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

pub(crate) fn mentions_path_attribute(bytes: &[u8]) -> bool {
    bytes.iter().enumerate().any(|(at, byte)| {
        *byte == b'#' && {
            let rest = bytes[at + 1..]
                .iter()
                .copied()
                .filter(|byte| !byte.is_ascii_whitespace() && *byte != b'!');
            let head: Vec<u8> = rest.take(9).collect();
            head.starts_with(b"[path") || head.starts_with(b"[cfg_attr")
        }
    })
}
