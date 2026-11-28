// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Value {
    Text(String),
    Flag(bool),
    List(Vec<Value>),
    Dict(Vec<(String, Value)>),
}

impl Value {
    pub(super) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Dict(pairs) => pairs.iter().find(|(name, _)| name == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(super) fn keys(&self) -> Vec<&str> {
        match self {
            Self::Dict(pairs) => pairs.iter().map(|(name, _)| name.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }
}

enum Token {
    Open(String),
    Close(String),
    Empty(String),
    Text(String),
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let before = &rest[..start];
        if !before.trim().is_empty() {
            out.push(Token::Text(unescape(before)));
        }
        let end = start + rest[start..].find('>')?;
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        let name = |tag: &str| tag.split_whitespace().next().unwrap_or_default().to_owned();
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        out.push(if let Some(tag) = tag.strip_suffix('/') {
            Token::Empty(name(tag))
        } else if let Some(tag) = tag.strip_prefix('/') {
            Token::Close(name(tag))
        } else {
            Token::Open(name(tag))
        });
    }
    Some(out)
}

pub(super) fn parse(text: &str) -> Option<Value> {
    let mut stream = tokens(text)?.into_iter().peekable();
    let Some(Token::Open(root)) = stream.next() else {
        return None;
    };
    if root != "plist" {
        return None;
    }
    let value = value(&mut stream)?;
    matches!(stream.next(), Some(Token::Close(name)) if name == "plist").then_some(value)
}

fn value(stream: &mut std::iter::Peekable<std::vec::IntoIter<Token>>) -> Option<Value> {
    match stream.next()? {
        Token::Empty(name) => match name.as_str() {
            "true" => Some(Value::Flag(true)),
            "false" => Some(Value::Flag(false)),
            _ => None,
        },
        Token::Open(name) if name == "string" => {
            let text = match stream.next()? {
                Token::Text(text) => {
                    matches!(stream.next()?, Token::Close(close) if close == "string")
                        .then_some(text)?
                }
                Token::Close(close) if close == "string" => String::new(),
                _ => return None,
            };
            Some(Value::Text(text))
        }
        Token::Open(name) if name == "array" => {
            let mut items = Vec::new();
            while !matches!(stream.peek()?, Token::Close(close) if close == "array") {
                items.push(value(stream)?);
            }
            stream.next();
            Some(Value::List(items))
        }
        Token::Open(name) if name == "dict" => {
            let mut pairs = Vec::new();
            while !matches!(stream.peek()?, Token::Close(close) if close == "dict") {
                let (Token::Open(key), Token::Text(name), Token::Close(_)) =
                    (stream.next()?, stream.next()?, stream.next()?)
                else {
                    return None;
                };
                if key != "key" {
                    return None;
                }
                pairs.push((name, value(stream)?));
            }
            stream.next();
            Some(Value::Dict(pairs))
        }
        _ => None,
    }
}
