// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Platform {
    triple: String,
    unknown: bool,
    names: BTreeSet<String>,
    pairs: BTreeSet<(String, String)>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Platforms {
    pub host: Platform,
    pub targets: Vec<Platform>,
}

impl Platforms {
    #[must_use]
    pub fn holds_for_host(&self, predicate: &str) -> bool {
        self.host.holds(predicate)
    }

    #[must_use]
    pub fn holds_for_ordinary_edge(&self, predicate: &str) -> bool {
        self.host.holds(predicate) || self.targets.iter().any(|target| target.holds(predicate))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Text(String),
    Open,
    Close,
    Comma,
    Equals,
}

enum Expression {
    Name(String),
    Pair(String, String),
    All(Vec<Expression>),
    Any(Vec<Expression>),
    Not(Box<Expression>),
}

const FIXED_NAMES: &[&str] = &[
    "unix",
    "windows",
    "target_env",
    "target_vendor",
    "target_abi",
];

const FIXED_KEYS: &[&str] = &[
    "target_os",
    "target_family",
    "target_arch",
    "target_env",
    "target_vendor",
    "target_endian",
    "target_pointer_width",
    "target_abi",
    "target_has_atomic",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Truth {
    Yes,
    No,
    Unknown,
}

impl Truth {
    fn of(value: bool) -> Self {
        if value { Self::Yes } else { Self::No }
    }

    fn not(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
            Self::Unknown => Self::Unknown,
        }
    }
}

impl Platform {
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            unknown: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn parse(triple: &str, printed: &str) -> Self {
        let mut platform = Self {
            triple: triple.to_owned(),
            ..Self::default()
        };
        for line in printed.lines() {
            match line.split_once('=') {
                Some((key, value)) => {
                    platform.pairs.insert((
                        key.trim().to_owned(),
                        value.trim().trim_matches('"').to_owned(),
                    ));
                }
                None if !line.trim().is_empty() => {
                    platform.names.insert(line.trim().to_owned());
                }
                None => {}
            }
        }
        platform
    }

    #[must_use]
    pub fn holds(&self, predicate: &str) -> bool {
        let predicate = predicate.trim();
        let Some(rest) = predicate.strip_prefix("cfg(") else {
            return self.unknown || predicate == self.triple;
        };
        let Some(inner) = rest.strip_suffix(')') else {
            return true;
        };
        let mut tokens = tokens(inner).into_iter().peekable();
        match expression(&mut tokens) {
            Some(parsed) if tokens.peek().is_none() => self.evaluate(&parsed) != Truth::No,
            _ => true,
        }
    }

    fn evaluate(&self, expression: &Expression) -> Truth {
        match expression {
            Expression::Name(name) if name == "true" => Truth::Yes,
            Expression::Name(name) if name == "false" => Truth::No,
            Expression::Name(_) | Expression::Pair(..) if self.unknown => Truth::Unknown,
            Expression::Name(name) if FIXED_NAMES.contains(&name.as_str()) => {
                Truth::of(self.names.contains(name))
            }
            Expression::Pair(key, value) if FIXED_KEYS.contains(&key.as_str()) => {
                Truth::of(self.pairs.contains(&(key.clone(), value.clone())))
            }
            Expression::Name(_) | Expression::Pair(..) => Truth::Unknown,
            Expression::All(parts) => {
                parts
                    .iter()
                    .fold(Truth::Yes, |all, part| match (all, self.evaluate(part)) {
                        (Truth::No, _) | (_, Truth::No) => Truth::No,
                        (Truth::Unknown, _) | (_, Truth::Unknown) => Truth::Unknown,
                        _ => Truth::Yes,
                    })
            }
            Expression::Any(parts) => {
                parts
                    .iter()
                    .fold(Truth::No, |any, part| match (any, self.evaluate(part)) {
                        (Truth::Yes, _) | (_, Truth::Yes) => Truth::Yes,
                        (Truth::Unknown, _) | (_, Truth::Unknown) => Truth::Unknown,
                        _ => Truth::No,
                    })
            }
            Expression::Not(part) => self.evaluate(part).not(),
        }
    }
}

fn tokens(text: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' => found.push(Token::Open),
            ')' => found.push(Token::Close),
            ',' => found.push(Token::Comma),
            '=' => found.push(Token::Equals),
            '"' => {
                let mut value = String::new();
                for next in chars.by_ref() {
                    if next == '"' {
                        break;
                    }
                    value.push(next);
                }
                found.push(Token::Text(value));
            }
            c if c.is_whitespace() => {}
            c => {
                let mut word = String::from(c);
                while let Some(next) =
                    chars.next_if(|n| !"(),=\"".contains(*n) && !n.is_whitespace())
                {
                    word.push(next);
                }
                found.push(Token::Word(word));
            }
        }
    }
    found
}

type Stream = std::iter::Peekable<std::vec::IntoIter<Token>>;

fn expression(tokens: &mut Stream) -> Option<Expression> {
    let Token::Word(word) = tokens.next()? else {
        return None;
    };
    match tokens.peek() {
        Some(Token::Open) => {
            tokens.next();
            let mut parts = Vec::new();
            while tokens.peek() != Some(&Token::Close) {
                parts.push(expression(tokens)?);
                if tokens.peek() == Some(&Token::Comma) {
                    tokens.next();
                }
            }
            tokens.next();
            match word.as_str() {
                "all" => Some(Expression::All(parts)),
                "any" => Some(Expression::Any(parts)),
                "not" if parts.len() == 1 => parts.pop().map(|p| Expression::Not(Box::new(p))),
                _ => None,
            }
        }
        Some(Token::Equals) => {
            tokens.next();
            match tokens.next()? {
                Token::Text(value) => Some(Expression::Pair(word, value)),
                _ => None,
            }
        }
        _ => Some(Expression::Name(word)),
    }
}
