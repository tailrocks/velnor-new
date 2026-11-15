// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Scalar(String),
    Seq(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    pub(crate) fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Self::Map(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn at(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self, |node, key| node.get(key))
    }

    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::Scalar(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn keys(&self) -> Vec<&str> {
        match self {
            Self::Map(items) => items.iter().map(|(key, _)| key.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn items(&self) -> &[Node] {
        match self {
            Self::Seq(items) => items,
            _ => &[],
        }
    }

    pub(crate) fn texts(&self) -> Vec<&str> {
        self.items().iter().filter_map(Node::text).collect()
    }

    pub(crate) fn scalars(&self) -> Vec<&str> {
        match self {
            Self::Scalar(text) => vec![text],
            Self::Seq(items) => items.iter().flat_map(Node::scalars).collect(),
            Self::Map(items) => items
                .iter()
                .flat_map(|(_, value)| value.scalars())
                .collect(),
        }
    }

    pub(crate) fn has_key_anywhere(&self, wanted: &str) -> bool {
        match self {
            Self::Scalar(_) => false,
            Self::Seq(items) => items.iter().any(|item| item.has_key_anywhere(wanted)),
            Self::Map(items) => items
                .iter()
                .any(|(key, value)| key == wanted || value.has_key_anywhere(wanted)),
        }
    }
}

type Line = (usize, String);

pub(crate) fn parse(text: &str) -> Result<Node, String> {
    let mut lines: Vec<Line> = text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| {
            let body = line.trim_start();
            (line.len() - body.len(), body.to_owned())
        })
        .collect();
    let first = lines.first().ok_or("empty document")?.0;
    let (node, used) = block(&mut lines, 0, first)?;
    if used == lines.len() {
        Ok(node)
    } else {
        Err(format!("unparsed line {}: {}", used + 1, lines[used].1))
    }
}

fn block(lines: &mut Vec<Line>, start: usize, indent: usize) -> Result<(Node, usize), String> {
    if lines[start].1.starts_with("- ") {
        sequence(lines, start, indent)
    } else {
        mapping(lines, start, indent)
    }
}

fn mapping(lines: &mut Vec<Line>, start: usize, indent: usize) -> Result<(Node, usize), String> {
    let mut items = Vec::new();
    let mut at = start;
    while at < lines.len() && lines[at].0 == indent && !lines[at].1.starts_with("- ") {
        let (key, value) = split(&lines[at].1)?;
        if value.is_empty() {
            let nested = lines
                .get(at + 1)
                .filter(|next| next.0 > indent)
                .map(|n| n.0);
            if let Some(child) = nested {
                let (node, next) = block(lines, at + 1, child)?;
                items.push((key, node));
                at = next;
            } else {
                items.push((key, Node::Scalar(String::new())));
                at += 1;
            }
        } else {
            items.push((key, scalar(&value)?));
            at += 1;
        }
    }
    Ok((Node::Map(items), at))
}

fn sequence(lines: &mut Vec<Line>, start: usize, indent: usize) -> Result<(Node, usize), String> {
    let mut items = Vec::new();
    let mut at = start;
    while at < lines.len() && lines[at].0 == indent && lines[at].1.starts_with("- ") {
        let rest = lines[at].1[2..].to_owned();
        if !rest.starts_with('\'') && (rest.contains(": ") || rest.ends_with(':')) {
            lines[at] = (indent + 2, rest);
            let (node, next) = mapping(lines, at, indent + 2)?;
            items.push(node);
            at = next;
        } else {
            items.push(scalar(&rest)?);
            at += 1;
        }
    }
    Ok((Node::Seq(items), at))
}

fn split(line: &str) -> Result<(String, String), String> {
    if let Some(key) = line.strip_suffix(':') {
        return Ok((key.to_owned(), String::new()));
    }
    let (key, value) = line
        .split_once(": ")
        .ok_or(format!("not a mapping line: {line}"))?;
    Ok((key.to_owned(), value.to_owned()))
}

fn scalar(value: &str) -> Result<Node, String> {
    match value {
        "{}" => return Ok(Node::Map(Vec::new())),
        "[]" => return Ok(Node::Seq(Vec::new())),
        _ => {}
    }
    if let Some(quoted) = value.strip_prefix('\'') {
        let mut text = String::new();
        let mut chars = quoted.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek()) {
                ('\'', Some('\'')) => {
                    text.push('\'');
                    chars.next();
                }
                ('\'', _) => return Ok(Node::Scalar(text)),
                (c, _) => text.push(c),
            }
        }
        return Err(format!("unterminated quote: {value}"));
    }
    Ok(Node::Scalar(
        value
            .split_once(" #")
            .map_or(value, |(text, _)| text)
            .trim_end()
            .to_owned(),
    ))
}
