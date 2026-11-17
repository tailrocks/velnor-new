// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Text(String),
    Flag(bool),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Self::Text(text) => !text.is_empty(),
            Self::Flag(flag) => *flag,
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Flag(flag) => flag.to_string(),
        }
    }
}

struct Parser<'a> {
    chars: Vec<char>,
    at: usize,
    context: &'a BTreeMap<&'a str, &'a str>,
}

pub(crate) fn evaluate(condition: &str, context: &BTreeMap<&str, &str>) -> Result<bool, String> {
    let mut parser = Parser {
        chars: condition.chars().collect(),
        at: 0,
        context,
    };
    let value = parser.or()?;
    parser.skip();
    if parser.at != parser.chars.len() {
        return Err(format!("trailing input in {condition}"));
    }
    Ok(value.truthy())
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip();
        let width = token.chars().count();
        let found: String = self.chars.iter().skip(self.at).take(width).collect();
        if found == token {
            self.at += width;
            return true;
        }
        false
    }

    fn or(&mut self) -> Result<Value, String> {
        let mut left = self.and()?;
        while self.eat("||") {
            let right = self.and()?;
            left = Value::Flag(left.truthy() || right.truthy());
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Value, String> {
        let mut left = self.equality()?;
        while self.eat("&&") {
            let right = self.equality()?;
            left = Value::Flag(left.truthy() && right.truthy());
        }
        Ok(left)
    }

    fn equality(&mut self) -> Result<Value, String> {
        let left = self.unary()?;
        for (token, expected) in [("==", true), ("!=", false)] {
            if self.eat(token) {
                let right = self.unary()?;
                let same = left.text().eq_ignore_ascii_case(&right.text());
                return Ok(Value::Flag(same == expected));
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Value, String> {
        self.skip();
        if self.chars.get(self.at) == Some(&'!') && self.chars.get(self.at + 1) != Some(&'=') {
            self.at += 1;
            return Ok(Value::Flag(!self.unary()?.truthy()));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Value, String> {
        if self.eat("(") {
            let inner = self.or()?;
            return if self.eat(")") {
                Ok(inner)
            } else {
                Err("missing )".to_owned())
            };
        }
        self.skip();
        if self.chars.get(self.at) == Some(&'\'') {
            return self.string();
        }
        let start = self.at;
        while self
            .chars
            .get(self.at)
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            self.at += 1;
        }
        let name: String = self.chars[start..self.at].iter().collect();
        if name.is_empty() {
            return Err(format!("unexpected input at {start}"));
        }
        if self.eat("(") {
            return self.call(&name);
        }
        self.context
            .get(name.as_str())
            .map(|value| Value::Text((*value).to_owned()))
            .ok_or_else(|| format!("unknown context {name}"))
    }

    fn string(&mut self) -> Result<Value, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            match self.chars.get(self.at) {
                None => return Err("unterminated string".to_owned()),
                Some('\'') if self.chars.get(self.at + 1) == Some(&'\'') => {
                    out.push('\'');
                    self.at += 2;
                }
                Some('\'') => {
                    self.at += 1;
                    return Ok(Value::Text(out));
                }
                Some(c) => {
                    out.push(*c);
                    self.at += 1;
                }
            }
        }
    }

    fn call(&mut self, name: &str) -> Result<Value, String> {
        let mut args = vec![self.or()?];
        while self.eat(",") {
            args.push(self.or()?);
        }
        if !self.eat(")") {
            return Err("missing ) after arguments".to_owned());
        }
        match (name, args.as_slice()) {
            ("startsWith", [text, prefix]) => Ok(Value::Flag(
                text.text()
                    .to_lowercase()
                    .starts_with(&prefix.text().to_lowercase()),
            )),
            ("format", [template, rest @ ..]) => {
                let mut out = template.text();
                for (index, arg) in rest.iter().enumerate() {
                    out = out.replace(&format!("{{{index}}}"), &arg.text());
                }
                Ok(Value::Text(out))
            }
            _ => Err(format!("unsupported function {name}")),
        }
    }
}
