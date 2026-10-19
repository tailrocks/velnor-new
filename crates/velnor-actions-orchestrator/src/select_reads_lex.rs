//! Minimal Rust token stream for the out-of-directory read scan.
//!
//! The scan needs identifiers, string-literal values, and punctuation,
//! with comments, character literals, lifetimes, and numbers skipped.
//! It is not a parser: escapes inside string literals are kept verbatim
//! (paths never need them), and malformed input simply ends the stream.

/// One lexical token the read scan inspects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    /// Identifier or keyword.
    Ident(String),
    /// String-literal body (normal, raw, byte, or C string).
    Str(String),
    /// Any other single punctuation character.
    Punct(char),
}

/// Tokenize Rust source text.
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        if ch.is_whitespace() {
            at += 1;
        } else if ch == '/' && chars.get(at + 1) == Some(&'/') {
            at = skip_line(&chars, at);
        } else if ch == '/' && chars.get(at + 1) == Some(&'*') {
            at = skip_block(&chars, at + 2);
        } else if ch == '"' {
            let (body, next) = quoted(&chars, at + 1);
            tokens.push(Token::Str(body));
            at = next;
        } else if ch == '\'' {
            at = skip_quote(&chars, at);
        } else if ch.is_ascii_digit() {
            at = skip_ident_chars(&chars, at);
        } else if ch.is_alphabetic() || ch == '_' {
            let end = skip_ident_chars(&chars, at);
            let word: String = chars.get(at..end).unwrap_or_default().iter().collect();
            if let Some((body, next)) = prefixed_string(&chars, &word, end) {
                tokens.push(Token::Str(body));
                at = next;
            } else {
                tokens.push(Token::Ident(word));
                at = end;
            }
        } else {
            tokens.push(Token::Punct(ch));
            at += 1;
        }
    }
    tokens
}

/// End of identifier (or number) characters starting at `at`.
fn skip_ident_chars(chars: &[char], mut at: usize) -> usize {
    while chars
        .get(at)
        .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
    {
        at += 1;
    }
    at
}

/// Index after a line comment.
fn skip_line(chars: &[char], mut at: usize) -> usize {
    while chars.get(at).is_some_and(|ch| *ch != '\n') {
        at += 1;
    }
    at
}

/// Index after a (nested) block comment whose opener ends before `at`.
fn skip_block(chars: &[char], mut at: usize) -> usize {
    let mut depth = 1usize;
    while depth > 0 {
        match (chars.get(at), chars.get(at + 1)) {
            (Some('/'), Some('*')) => {
                depth += 1;
                at += 2;
            }
            (Some('*'), Some('/')) => {
                depth -= 1;
                at += 2;
            }
            (Some(_), _) => at += 1,
            (None, _) => return at,
        }
    }
    at
}

/// Body and next index of a `"..."` literal whose quote ends before `at`.
fn quoted(chars: &[char], mut at: usize) -> (String, usize) {
    let mut body = String::new();
    while let Some(&ch) = chars.get(at) {
        match ch {
            '\\' => {
                if let Some(&escaped) = chars.get(at + 1) {
                    body.push('\\');
                    body.push(escaped);
                }
                at += 2;
            }
            '"' => return (body, at + 1),
            _ => {
                body.push(ch);
                at += 1;
            }
        }
    }
    (body, at)
}

/// Raw, byte, or C string literal after prefix `word` ending at `at`.
fn prefixed_string(chars: &[char], word: &str, at: usize) -> Option<(String, usize)> {
    match word {
        "b" | "c" if chars.get(at) == Some(&'"') => Some(quoted(chars, at + 1)),
        "r" | "br" | "cr" => raw(chars, at),
        _ => None,
    }
}

/// Raw string `#*"..."#*` starting at the first `#` or quote.
fn raw(chars: &[char], mut at: usize) -> Option<(String, usize)> {
    let mut hashes = 0usize;
    while chars.get(at) == Some(&'#') {
        hashes += 1;
        at += 1;
    }
    if chars.get(at) != Some(&'"') {
        return None;
    }
    at += 1;
    let start = at;
    while at < chars.len() {
        if chars.get(at) == Some(&'"')
            && (1..=hashes).all(|offset| chars.get(at + offset) == Some(&'#'))
        {
            let body = chars.get(start..at).unwrap_or_default().iter().collect();
            return Some((body, at + 1 + hashes));
        }
        at += 1;
    }
    Some((chars.get(start..).unwrap_or_default().iter().collect(), at))
}

/// Skip a character literal or a lifetime starting at quote index `at`.
fn skip_quote(chars: &[char], at: usize) -> usize {
    if chars.get(at + 1) == Some(&'\\') {
        // Skip the escaped character itself: `'\''` must not end at it.
        let mut end = at + 3;
        while chars.get(end).is_some_and(|ch| *ch != '\'') {
            end += 1;
        }
        return end + 1;
    }
    if chars.get(at + 2) == Some(&'\'') {
        return at + 3;
    }
    skip_ident_chars(chars, at + 1)
}

#[cfg(test)]
mod tests {
    use super::{Token, tokenize};

    fn strs(text: &str) -> Vec<String> {
        tokenize(text)
            .into_iter()
            .filter_map(|token| match token {
                Token::Str(body) => Some(body),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn comments_chars_and_lifetimes_hide_no_literals() {
        let text = "// \"a\"\n/* \"b\" /* \"c\" */ */ fn f<'a>(x: &'a str) { let q = '\"'; let e = '\\''; g(\"d\") }";
        assert_eq!(strs(text), vec!["d".to_owned()]);
    }

    #[test]
    fn escaped_quote_chars_keep_string_parity() {
        let text = "let q = ('\\'','\"'); include_str!(\"x\");";
        assert_eq!(strs(text), vec!["x".to_owned()]);
    }

    #[test]
    fn raw_byte_and_c_strings_are_literals() {
        let text = "r#\"x\"y\"# br\"z\" b\"w\" c\"v\" r\"u\"";
        assert_eq!(strs(text), vec!["x\"y", "z", "w", "v", "u"]);
    }

    #[test]
    fn identifiers_and_punctuation_survive() {
        assert_eq!(
            tokenize("include_str!(\"a\")"),
            vec![
                Token::Ident("include_str".to_owned()),
                Token::Punct('!'),
                Token::Punct('('),
                Token::Str("a".to_owned()),
                Token::Punct(')'),
            ]
        );
    }
}
