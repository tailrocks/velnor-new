use super::{QuoteStyle, Token, TokenKind};

pub(super) fn tokenize(sql: &str) -> Option<Vec<Token<'_>>> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let start = index;
        let kind = match bytes[index] {
            b'\'' => {
                index = quoted_end(bytes, index, b'\'')?.0;
                TokenKind::String
            }
            b'"' => quoted_identifier(bytes, sql, &mut index, QuoteStyle::Double, b'"')?,
            b'`' => quoted_identifier(bytes, sql, &mut index, QuoteStyle::Backtick, b'`')?,
            b'[' => quoted_identifier(bytes, sql, &mut index, QuoteStyle::Bracket, b']')?,
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                TokenKind::Word(&sql[start..index])
            }
            byte if byte.is_ascii() => {
                index += 1;
                TokenKind::Symbol(byte)
            }
            _ => {
                let character = sql[index..].chars().next()?;
                index += character.len_utf8();
                TokenKind::Other
            }
        };
        tokens.push(Token {
            start,
            end: index,
            kind,
        });
    }
    Some(tokens)
}

pub(super) fn has_identifier_adjacency(tokens: &[Token<'_>]) -> bool {
    tokens.windows(2).any(|pair| {
        let [previous, next] = pair else {
            return false;
        };
        let adjacent = previous.end == next.start;
        adjacent && is_identifier_or_literal(previous.kind) && is_identifier_or_literal(next.kind)
    })
}

fn is_identifier_or_literal(kind: TokenKind<'_>) -> bool {
    matches!(
        kind,
        TokenKind::Word(_) | TokenKind::QuotedIdentifier { .. } | TokenKind::String
    )
}

fn quoted_identifier<'a>(
    bytes: &[u8],
    sql: &'a str,
    index: &mut usize,
    style: QuoteStyle,
    closing: u8,
) -> Option<TokenKind<'a>> {
    let (end, escaped) = quoted_end(bytes, *index, closing)?;
    if escaped {
        return None;
    }
    let name =
        simple_identifier(&bytes[*index + 1..end - 1]).and_then(|()| sql.get(*index + 1..end - 1));
    *index = end;
    Some(TokenKind::QuotedIdentifier { name, style })
}

fn quoted_end(bytes: &[u8], start: usize, closing: u8) -> Option<(usize, bool)> {
    let mut index = start + 1;
    let mut escaped = false;
    while index < bytes.len() {
        if bytes[index] != closing {
            index += 1;
        } else if bytes.get(index + 1) == Some(&closing) {
            escaped = true;
            index += 2;
        } else {
            return Some((index + 1, escaped));
        }
    }
    None
}

fn simple_identifier(bytes: &[u8]) -> Option<()> {
    if bytes.is_empty()
        || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        None
    } else {
        Some(())
    }
}
