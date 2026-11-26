// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use globset::{GlobBuilder, GlobMatcher};
use std::io;
use std::path::{Component, Path};

#[derive(Debug, thiserror::Error)]
pub(crate) enum GlobError {
    #[error("pattern cannot be empty")]
    EmptyPattern,
    #[error("Invalid pattern '{0}'. Relative pathing '.' and '..' is not allowed.")]
    RelativePathing(String),
    #[error("Invalid pattern '{0}'")]
    InvalidPattern(String),
    #[error("Unable to determine HOME directory")]
    NoHome,
    #[error("Expected HOME directory to be a rooted path. Actual '{0}'")]
    HomeNotRooted(String),
    #[error("{path}: {source}")]
    Io { path: String, source: io::Error },
}

impl GlobError {
    pub(crate) fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

pub(crate) fn components_of(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone)]
pub(crate) enum Seg {
    GlobStar,
    Any,
    Literal(String),
    Glob {
        matcher: GlobMatcher,
        literal: Option<String>,
    },
}

impl Seg {
    pub(crate) fn is_match(&self, name: &str) -> bool {
        match self {
            Seg::GlobStar | Seg::Any => true,
            Seg::Literal(literal) => literal == name,
            Seg::Glob { matcher, .. } => matcher.is_match(name),
        }
    }

    pub(crate) fn literal(&self) -> Option<&str> {
        match self {
            Seg::GlobStar | Seg::Any => None,
            Seg::Literal(literal) => Some(literal),
            Seg::Glob { literal, .. } => literal.as_deref(),
        }
    }

    fn compile(part: &str) -> Result<Self, GlobError> {
        if part == "**" {
            return Ok(Seg::GlobStar);
        }
        let matcher = GlobBuilder::new(&escape_segment(part))
            .allow_unclosed_class(true)
            .build()
            .map_err(|_| GlobError::InvalidPattern(part.to_owned()))?
            .compile_matcher();
        Ok(Seg::Glob {
            matcher,
            literal: Some(literal_of(part)).filter(|literal| !literal.is_empty()),
        })
    }
}

#[derive(Debug)]
pub(crate) struct Pattern {
    pub(crate) negate: bool,
    pub(crate) trailing: bool,
    pub(crate) segs: Vec<Seg>,
}

impl Pattern {
    pub(crate) fn parse(
        line: &str,
        workspace: &[String],
        home: Option<&Path>,
    ) -> Result<Self, GlobError> {
        let (negate, text) = strip_negation(line);
        if text.is_empty() {
            return Err(GlobError::EmptyPattern);
        }
        let normalized = collapse_slashes(text);
        let rooted = normalized.starts_with('/');
        let trailing = normalized.ends_with('/');
        let body = normalized.trim_matches('/');
        let parts: Vec<&str> = if body.is_empty() {
            Vec::new()
        } else {
            body.split('/').collect()
        };
        let relative_pathing = parts.iter().enumerate().any(|(index, part)| {
            let literal = literal_of(part);
            literal == ".." || (literal == "." && (rooted || index > 0))
        });
        if relative_pathing {
            return Err(GlobError::RelativePathing(text.to_owned()));
        }
        let first = parts.first().copied();
        let (mut segs, rest): (Vec<Seg>, &[&str]) = if rooted {
            (Vec::new(), &parts[..])
        } else if first == Some("~") {
            (home_segments(home)?, &parts[1..])
        } else {
            let anchored = workspace.iter().cloned().map(Seg::Literal).collect();
            (anchored, &parts[usize::from(first == Some("."))..])
        };
        for part in rest {
            segs.push(Seg::compile(part)?);
        }
        Ok(Self {
            negate,
            trailing,
            segs,
        })
    }

    pub(crate) fn implicit_descendants(&self) -> Vec<Seg> {
        let mut segs = self.segs.clone();
        segs.extend([Seg::Any, Seg::GlobStar]);
        segs
    }
}

fn home_segments(home: Option<&Path>) -> Result<Vec<Seg>, GlobError> {
    let home = home
        .filter(|home| !home.as_os_str().is_empty())
        .ok_or(GlobError::NoHome)?;
    if !home.is_absolute() {
        return Err(GlobError::HomeNotRooted(home.display().to_string()));
    }
    Ok(components_of(home).into_iter().map(Seg::Literal).collect())
}

pub(crate) fn align(segs: &[Seg], path: &[String]) -> Vec<bool> {
    let mut live = vec![false; segs.len() + 1];
    live[0] = true;
    skip_globstars(segs, &mut live);
    for name in path {
        let mut next = vec![false; segs.len() + 1];
        for (index, seg) in segs.iter().enumerate() {
            if live[index] && seg.is_match(name) {
                next[if matches!(seg, Seg::GlobStar) {
                    index
                } else {
                    index + 1
                }] = true;
            }
        }
        skip_globstars(segs, &mut next);
        live = next;
        if !live.iter().any(|alive| *alive) {
            break;
        }
    }
    live
}

fn skip_globstars(segs: &[Seg], live: &mut [bool]) {
    for (index, seg) in segs.iter().enumerate() {
        if live[index] && matches!(seg, Seg::GlobStar) {
            live[index + 1] = true;
        }
    }
}

fn is_space(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

pub(crate) fn trim_line(line: &str) -> &str {
    line.trim_matches(is_space)
}

fn strip_negation(line: &str) -> (bool, &str) {
    let mut negate = false;
    let mut text = trim_line(line);
    while let Some(rest) = text.strip_prefix('!') {
        negate = !negate;
        text = trim_line(rest);
    }
    (negate, text)
}

fn collapse_slashes(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for character in text.chars() {
        if character != '/' || !collapsed.ends_with('/') {
            collapsed.push(character);
        }
    }
    collapsed
}

fn class_end(chars: &[char], open: usize) -> Option<usize> {
    let mut index = open + 1;
    if matches!(chars.get(index), Some('!' | '^')) {
        index += 1;
    }
    if chars.get(index) == Some(&']') {
        index += 1;
    }
    chars[index.min(chars.len())..]
        .iter()
        .position(|character| *character == ']')
        .map(|offset| index + offset)
}

fn escape_segment(part: &str) -> String {
    let chars: Vec<char> = part.chars().collect();
    let mut escaped = String::with_capacity(part.len());
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        match character {
            '\\' if index + 1 < chars.len() => {
                escaped.push(character);
                escaped.push(chars[index + 1]);
                index += 2;
            }
            '\\' => {
                escaped.push_str("\\\\");
                index += 1;
            }
            '{' | '}' => {
                escaped.extend(['[', character, ']']);
                index += 1;
            }
            '[' => {
                if let Some(end) = class_end(&chars, index) {
                    escaped.extend(&chars[index..=end]);
                    index = end + 1;
                } else {
                    escaped.push(character);
                    index += 1;
                }
            }
            '*' => {
                escaped.push('*');
                while chars.get(index) == Some(&'*') {
                    index += 1;
                }
            }
            _ => {
                escaped.push(character);
                index += 1;
            }
        }
    }
    escaped
}

pub(crate) fn literal_of(segment: &str) -> String {
    let chars: Vec<char> = segment.chars().collect();
    let mut literal = String::new();
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if character == '\\' && index + 1 < chars.len() {
            literal.push(chars[index + 1]);
            index += 2;
            continue;
        }
        if matches!(character, '*' | '?') {
            return String::new();
        }
        if character == '[' && index + 1 < chars.len() {
            let mut set = String::new();
            let mut closed = None;
            let mut cursor = index + 1;
            while cursor < chars.len() {
                match chars[cursor] {
                    '\\' if cursor + 1 < chars.len() => {
                        set.push(chars[cursor + 1]);
                        cursor += 2;
                    }
                    ']' => {
                        closed = Some(cursor);
                        break;
                    }
                    other => {
                        set.push(other);
                        cursor += 1;
                    }
                }
            }
            if let Some(end) = closed {
                if set.chars().count() > 1 {
                    return String::new();
                }
                if !set.is_empty() {
                    literal.push_str(&set);
                    index = end + 1;
                    continue;
                }
            }
        }
        literal.push(character);
        index += 1;
    }
    literal
}
