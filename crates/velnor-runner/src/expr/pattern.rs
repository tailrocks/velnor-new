// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::segment::Segment;
use std::io;
use std::path::{Component, Path};

const MAX_PATTERN_UNITS: usize = 65_536;

#[derive(Debug, thiserror::Error)]
pub(crate) enum GlobError {
    #[error("pattern cannot be empty")]
    EmptyPattern,
    #[error("Invalid pattern '{0}'. Relative pathing '.' and '..' is not allowed.")]
    RelativePathing(String),
    #[error(
        "pattern segment '{0}' holds a named or modified regular expression group inside a character class, which is not supported"
    )]
    UnsupportedClass(String),
    #[error("pattern is too long")]
    PatternTooLong,
    #[error("pattern holds more than {0} nested unclosed character classes")]
    ClassNestingTooDeep(usize),
    #[error(
        "pattern segment '{0}' compiles to a regular expression that V8 rejects: Regular expression too large"
    )]
    RegexTooLarge(String),
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
    pub(crate) fn is_match(&self, name: &str) -> Result<bool, GlobError> {
        match self {
            Seg::GlobStar | Seg::Any => Ok(true),
            Seg::Literal(literal) => Ok(literal == name),
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
    rooted: String,
    leading_dot: bool,
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
        let (mut segs, rest, root): (Vec<Seg>, &[&str], String) = if rooted {
            (Vec::new(), &parts[..], String::from("/"))
        } else if first == Some("~") {
            let segs = home_segments(home).map_err(|error| match (error, home) {
                (GlobError::RelativePathing(_), Some(home)) => {
                    GlobError::RelativePathing(text.replacen('~', &home.display().to_string(), 1))
                }
                (error, _) => error,
            })?;
            let root = home.map(|home| glob_escape(&home.display().to_string()));
            (segs, &parts[1..], root.unwrap_or_default())
        } else {
            let anchored = workspace.iter().cloned().map(Seg::Literal).collect();
            let cwd = format!("/{}", workspace.join("/"));
            (
                anchored,
                &parts[usize::from(first == Some("."))..],
                glob_escape(&cwd),
            )
        };
        let rooted_text = rooted_text(root, rest);
        if units(&rooted_text) > MAX_PATTERN_UNITS {
            return Err(GlobError::PatternTooLong);
        }
        for part in rest {
            segs.push(Seg::compile(part)?);
        }
        let leading_dot = !rooted
            && first.is_some_and(|part| part != "." && part != "~" && literal_of(part) == ".");
        Ok(Self {
            negate,
            trailing,
            segs,
            rooted: rooted_text,
            leading_dot,
        })
    }

    pub(crate) fn implicit_descendants(&self) -> Result<Vec<Seg>, GlobError> {
        let separator = if self.rooted.ends_with('/') { "" } else { "/" };
        let rebuilt = format!("{}{separator}**", self.rooted);
        if self.leading_dot {
            return Err(GlobError::RelativePathing(rebuilt));
        }
        if units(&rebuilt) > MAX_PATTERN_UNITS {
            return Err(GlobError::PatternTooLong);
        }
        let mut segs = self.segs.clone();
        segs.extend([Seg::Any, Seg::GlobStar]);
        Ok(segs)
    }
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

fn rooted_text(root: String, rest: &[&str]) -> String {
    let mut text = root;
    for part in rest {
        if !text.ends_with('/') {
            text.push('/');
        }
        text.push_str(part);
    }
    text
}

fn glob_escape(text: &str) -> String {
    let chars: Vec<char> = text.replace('\\', "\\\\").chars().collect();
    let mut escaped = String::with_capacity(chars.len());
    for (index, &character) in chars.iter().enumerate() {
        match character {
            '[' if closes_in_segment(&chars[index + 1..]) => escaped.push_str("[[]"),
            '?' => escaped.push_str("[?]"),
            '*' => escaped.push_str("[*]"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn closes_in_segment(rest: &[char]) -> bool {
    rest.iter()
        .take_while(|&&character| character != '/')
        .skip(1)
        .any(|&character| character == ']')
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

pub(crate) fn align(segs: &[Seg], path: &[String]) -> Result<Vec<bool>, GlobError> {
    let mut live = vec![false; segs.len() + 1];
    live[0] = true;
    skip_globstars(segs, &mut live);
    for name in path {
        let mut next = vec![false; segs.len() + 1];
        for (index, seg) in segs.iter().enumerate() {
            if live[index] && seg.is_match(name)? {
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
    Ok(live)
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

fn class_closers(chars: &[char]) -> Vec<usize> {
    let mut closers = Vec::new();
    let mut backslashes = 0_usize;
    for (index, &character) in chars.iter().enumerate() {
        if character == ']' && backslashes.is_multiple_of(2) {
            closers.push(index);
        }
        backslashes = if character == '\\' {
            backslashes + 1
        } else {
            0
        };
    }
    closers
}

pub(crate) fn literal_of(segment: &str) -> String {
    let chars: Vec<char> = segment.chars().collect();
    let closers = class_closers(&chars);
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
            let first = closers.partition_point(|&closer| closer <= index);
            if let Some(&end) = closers.get(first) {
                let set = class_set(&chars[index + 1..end]);
                if set.encode_utf16().count() > 1 {
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

fn class_set(body: &[char]) -> String {
    let mut set = String::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if body[cursor] == '\\' && cursor + 1 < body.len() {
            set.push(body[cursor + 1]);
            cursor += 2;
        } else {
            set.push(body[cursor]);
            cursor += 1;
        }
    }
    set
}
