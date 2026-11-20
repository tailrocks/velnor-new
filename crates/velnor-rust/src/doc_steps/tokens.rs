// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::markdown::{block_lines, code_block, tested};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    Ident(&'a str),
    Punct(char),
    Doc { fenced: bool, tested: bool },
    Str(&'a str),
    Other,
}

pub(crate) fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        text,
        bytes: text.as_bytes(),
        at: if text.starts_with('\u{feff}') { 3 } else { 0 },
    };
    let mut found = Vec::new();
    while let Some(token) = lexer.next() {
        found.push(token);
    }
    found
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let byte = *self.bytes.get(self.at)?;
            let rest = &self.bytes[self.at..];
            let token = match byte {
                b'/' if rest.starts_with(b"//") => self.line_comment(),
                b'/' if rest.starts_with(b"/*") => self.block_comment(),
                b'"' => Some(self.string(self.at + 1)),
                b'\'' => Some(self.quote()),
                b'0'..=b'9' => Some(self.word(self.at, Token::Other)),
                byte if is_word(byte) => Some(self.identifier()),
                byte if byte.is_ascii_whitespace() => {
                    self.at += 1;
                    None
                }
                _ => {
                    let c = self.text[self.at..].chars().next()?;
                    self.at += c.len_utf8();
                    Some(Token::Punct(c))
                }
            };
            if token.is_some() {
                return token;
            }
        }
    }

    fn line_comment(&mut self) -> Option<Token<'a>> {
        let start = self.at;
        let end = self.text[start..]
            .find('\n')
            .map_or(self.text.len(), |n| start + n);
        self.at = end;
        let body = &self.text[start..end];
        let kind = body
            .get(..3)
            .filter(|kind| matches!(*kind, "///" | "//!"))?;
        if kind == "///" && body[3..].starts_with('/') {
            return None;
        }
        let mut lines = vec![&body[3..]];
        loop {
            let after = &self.text[self.at..];
            let next = after.trim_start();
            if !next.starts_with(kind) || (kind == "///" && next[3..].starts_with('/')) {
                break;
            }
            let from = self.at + (after.len() - next.len());
            let to = self.text[from..]
                .find('\n')
                .map_or(self.text.len(), |n| from + n);
            lines.push(&self.text[from + 3..to]);
            self.at = to;
        }
        Some(Token::Doc {
            fenced: lines.iter().any(|line| code_block(line)),
            tested: tested(lines.iter().copied()),
        })
    }

    fn block_comment(&mut self) -> Option<Token<'a>> {
        let start = self.at;
        let rest = &self.text[start..];
        let documented = rest.starts_with("/*!")
            || (rest.starts_with("/**") && !rest[3..].starts_with(['*', '/']));
        let mut depth = 0_usize;
        let mut at = start;
        while at < self.bytes.len() {
            if self.bytes[at..].starts_with(b"/*") {
                depth += 1;
                at += 2;
            } else if self.bytes[at..].starts_with(b"*/") {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    break;
                }
            } else {
                at += 1;
            }
        }
        self.at = at;
        documented.then(|| {
            let lines = block_lines(&self.text[start + 3..at]);
            Token::Doc {
                fenced: lines.iter().any(|line| code_block(line)),
                tested: tested(lines.iter().copied()),
            }
        })
    }

    fn string(&mut self, from: usize) -> Token<'a> {
        let mut at = from;
        let mut end = None;
        while let Some(&byte) = self.bytes.get(at) {
            match byte {
                b'\\' => at += 2,
                b'"' => {
                    end = Some(at);
                    at += 1;
                    break;
                }
                _ => at += 1,
            }
        }
        self.at = at.min(self.bytes.len());
        Token::Str(&self.text[from..end.unwrap_or(self.at).max(from)])
    }

    fn raw_string(&mut self, hashes: usize, from: usize) -> Token<'a> {
        let closing = format!("\"{}", "#".repeat(hashes));
        let end = self.text[from..]
            .find(&closing)
            .map_or(self.text.len(), |n| from + n);
        self.at = (end + closing.len()).min(self.text.len());
        Token::Str(&self.text[from..end])
    }

    fn quote(&mut self) -> Token<'a> {
        let after = self.at + 1;
        let first = self.text[after..].chars().next();
        let literal = match first {
            Some('\\') => true,
            Some(c) => self.bytes.get(after + c.len_utf8()) == Some(&b'\''),
            None => false,
        };
        if literal {
            let mut at = after;
            while let Some(&byte) = self.bytes.get(at) {
                at += 1;
                match byte {
                    b'\\' => at += 1,
                    b'\'' => break,
                    _ => {}
                }
            }
            self.at = at.min(self.bytes.len());
            Token::Other
        } else {
            self.word(after, Token::Other)
        }
    }

    fn word(&mut self, from: usize, token: Token<'a>) -> Token<'a> {
        let mut at = from;
        while self.bytes.get(at).is_some_and(|&byte| is_word(byte)) {
            at += 1;
        }
        self.at = at.max(self.at + 1);
        token
    }

    fn identifier(&mut self) -> Token<'a> {
        let start = self.at;
        let rest = &self.text[start..];
        let prefix = ["br", "cr", "r", "b", "c"]
            .into_iter()
            .find(|prefix| rest.starts_with(prefix));
        if let Some(prefix) = prefix {
            let after = start + prefix.len();
            let hashes = self.bytes[after..]
                .iter()
                .take_while(|&&b| b == b'#')
                .count();
            let opens = self.bytes.get(after + hashes) == Some(&b'"');
            if prefix.ends_with('r') && opens {
                return self.raw_string(hashes, after + hashes + 1);
            }
            if !prefix.ends_with('r') && hashes == 0 && opens {
                return self.string(after + 1);
            }
            if prefix == "b" && self.bytes.get(after) == Some(&b'\'') {
                self.at = after;
                return self.quote();
            }
            if prefix == "r"
                && hashes == 1
                && self.bytes.get(after + 1).is_some_and(|&b| is_word(b))
            {
                self.at = after + 1;
            }
        }
        let from = self.at;
        self.word(from, Token::Other);
        Token::Ident(&self.text[from..self.at])
    }
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}
