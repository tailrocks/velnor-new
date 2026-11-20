// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Str(String),
    List(Vec<Value>),
}

impl Value {
    #[must_use]
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(flag) => *flag,
            Self::Str(text) => !text.is_empty(),
            Self::List(_) => true,
        }
    }

    fn number(&self) -> Option<i64> {
        match self {
            Self::Null => Some(0),
            Self::Bool(flag) => Some(i64::from(*flag)),
            Self::Str(text) => text.parse().ok(),
            Self::List(_) => None,
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(flag) => flag.to_string(),
            Self::Str(text) => text.clone(),
            Self::List(_) => "Array".to_owned(),
        }
    }

    fn loosely_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Str(left), Self::Str(right)) => left.eq_ignore_ascii_case(right),
            (Self::Null, Self::Null) => true,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::List(_), _) | (_, Self::List(_)) => false,
            _ => self.number().is_some() && self.number() == other.number(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Text(String),
    Word(String),
    And,
    Or,
    Not,
    Equal,
    NotEqual,
    Open,
    Close,
    Comma,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let rest: String = chars[at..].iter().take(2).collect();
        match chars[at] {
            ' ' => at += 1,
            '(' => {
                out.push(Token::Open);
                at += 1;
            }
            ')' => {
                out.push(Token::Close);
                at += 1;
            }
            ',' => {
                out.push(Token::Comma);
                at += 1;
            }
            '\'' => {
                let mut value = String::new();
                at += 1;
                loop {
                    match (chars.get(at), chars.get(at + 1)) {
                        (Some('\''), Some('\'')) => {
                            value.push('\'');
                            at += 2;
                        }
                        (Some('\''), _) => {
                            at += 1;
                            break;
                        }
                        (Some(ch), _) => {
                            value.push(*ch);
                            at += 1;
                        }
                        (None, _) => return Err(format!("unterminated string in {text}")),
                    }
                }
                out.push(Token::Text(value));
            }
            _ if rest == "&&" => {
                out.push(Token::And);
                at += 2;
            }
            _ if rest == "||" => {
                out.push(Token::Or);
                at += 2;
            }
            _ if rest == "==" => {
                out.push(Token::Equal);
                at += 2;
            }
            _ if rest == "!=" => {
                out.push(Token::NotEqual);
                at += 2;
            }
            '!' => {
                out.push(Token::Not);
                at += 1;
            }
            ch if ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '*') => {
                let start = at;
                while at < chars.len()
                    && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '-' | '.' | '*'))
                {
                    at += 1;
                }
                out.push(Token::Word(chars[start..at].iter().collect()));
            }
            other => return Err(format!("unexpected {other:?} in {text}")),
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    resolve: &'a dyn Fn(&str) -> Result<Value, String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn take(&mut self, wanted: &Token) -> bool {
        let found = self.peek() == Some(wanted);
        if found {
            self.at += 1;
        }
        found
    }

    fn or(&mut self) -> Result<Value, String> {
        let mut value = self.and()?;
        while self.take(&Token::Or) {
            let next = self.and()?;
            if !value.truthy() {
                value = next;
            }
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<Value, String> {
        let mut value = self.comparison()?;
        while self.take(&Token::And) {
            let next = self.comparison()?;
            if value.truthy() {
                value = next;
            }
        }
        Ok(value)
    }

    fn comparison(&mut self) -> Result<Value, String> {
        let mut value = self.unary()?;
        loop {
            let equal = if self.take(&Token::Equal) {
                true
            } else if self.take(&Token::NotEqual) {
                false
            } else {
                return Ok(value);
            };
            let next = self.unary()?;
            value = Value::Bool(value.loosely_equals(&next) == equal);
        }
    }

    fn unary(&mut self) -> Result<Value, String> {
        if self.take(&Token::Not) {
            return Ok(Value::Bool(!self.unary()?.truthy()));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Value, String> {
        let token = self.peek().cloned().ok_or("the expression ends early")?;
        self.at += 1;
        match token {
            Token::Open => {
                let value = self.or()?;
                if self.take(&Token::Close) {
                    Ok(value)
                } else {
                    Err("a parenthesis is not closed".to_owned())
                }
            }
            Token::Text(text) => Ok(Value::Str(text)),
            Token::Word(word) if self.take(&Token::Open) => {
                let mut arguments = Vec::new();
                while !self.take(&Token::Close) {
                    arguments.push(self.or()?);
                    self.take(&Token::Comma);
                }
                call(&word, &arguments)
            }
            Token::Word(word) => Ok(match word.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                _ => return (self.resolve)(&word),
            }),
            other => Err(format!("unexpected {other:?}")),
        }
    }
}

fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match (name, arguments) {
        ("always" | "success", []) => Ok(Value::Bool(true)),
        ("cancelled" | "failure", []) => Ok(Value::Bool(false)),
        ("contains", [Value::List(items), wanted]) => Ok(Value::Bool(
            items.iter().any(|item| item.loosely_equals(wanted)),
        )),
        ("contains", [search, wanted]) => Ok(Value::Bool(
            search
                .text()
                .to_lowercase()
                .contains(&wanted.text().to_lowercase()),
        )),
        ("startsWith", [search, prefix]) => Ok(Value::Bool(
            search
                .text()
                .to_lowercase()
                .starts_with(&prefix.text().to_lowercase()),
        )),
        ("format", [pattern, rest @ ..]) => {
            let mut out = pattern.text();
            for (index, value) in rest.iter().enumerate() {
                out = out.replace(&format!("{{{index}}}"), &value.text());
            }
            Ok(Value::Str(out))
        }
        _ => Err(format!("{name} with {} arguments", arguments.len())),
    }
}

pub fn evaluate(
    text: &str,
    resolve: &dyn Fn(&str) -> Result<Value, String>,
) -> Result<Value, String> {
    let inner = text
        .trim()
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(text)
        .trim();
    let mut parser = Parser {
        tokens: tokens(inner)?,
        at: 0,
        resolve,
    };
    let value = parser.or()?;
    if parser.at == parser.tokens.len() {
        Ok(value)
    } else {
        Err(format!("trailing tokens in {text}"))
    }
}

#[must_use]
pub fn has_status_function(text: &str) -> bool {
    ["always()", "cancelled()", "success()", "failure()"]
        .iter()
        .any(|function| text.contains(function))
}
