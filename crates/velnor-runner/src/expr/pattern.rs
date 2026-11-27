// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::segment::Segment;
use std::io;
use std::path::{Component, Path};

#[derive(Debug, thiserror::Error)]
pub(crate) enum GlobError {
    #[error("pattern cannot be empty")]
    EmptyPattern,
    #[error("Invalid pattern '{0}'. Relative pathing '.' and '..' is not allowed.")]
    RelativePathing(String),
    #[error(
        "pattern segment '{0}' has a character class that starts with ']' and holds regular expression syntax, which is not supported"
    )]
    UnsupportedClass(String),
    #[error("pattern is too long")]
    PatternTooLong,
    #[error("{0}: not a regular file")]
    NotRegularFile(String),
    #[error("path '{0}' is not valid UTF-8")]
    NotUtf8(String),
    #[error("Unable to determine HOME directory")]
    NoHome,
    #[error("Expected HOME directory to be a rooted path. Actual '{0}'")]
    HomeNotRooted(String),
    #[error("the walk ran past its deadline")]
    TimedOut,
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

pub(crate) fn components_of(path: &Path) -> Result<Vec<String>, GlobError> {
    let text = path
        .to_str()
        .ok_or_else(|| GlobError::NotUtf8(path.display().to_string()))?;
    if text
        .split(std::path::is_separator)
        .any(|part| part == "." || part == "..")
    {
        return Err(GlobError::RelativePathing(text.to_owned()));
    }
    Ok(path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect())
}

#[derive(Debug, Clone)]
pub(crate) enum Seg {
    GlobStar,
    Any,
    Literal(String),
    Glob {
        matcher: Segment,
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

    pub(crate) fn compile(part: &str) -> Result<Self, GlobError> {
        if part == "**" {
            return Ok(Seg::GlobStar);
        }
        Ok(Seg::Glob {
            matcher: Segment::compile(part)?,
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
            let segs = home_segments(home).map_err(|error| match (error, home) {
                (GlobError::RelativePathing(_), Some(home)) => {
                    GlobError::RelativePathing(text.replacen('~', &home.display().to_string(), 1))
                }
                (error, _) => error,
            })?;
            (segs, &parts[1..])
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
    Ok(components_of(home)?.into_iter().map(Seg::Literal).collect())
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
    (character.is_whitespace() && character != '\u{85}') || character == '\u{feff}'
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
