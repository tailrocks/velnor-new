// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::js_class::{Class, parse_translated};
use super::pattern::GlobError;
use super::translate::translate;

const MAX_PATTERN_UNITS: usize = 65_536;
const BACKSLASH: u16 = b'\\' as u16;
const OPEN: u16 = b'[' as u16;
const CLOSE: u16 = b']' as u16;
const STAR: u16 = b'*' as u16;
const QUESTION: u16 = b'?' as u16;

#[derive(Debug, Clone)]
enum Item {
    Star,
    Unit(u16),
    Class(Class),
}

#[derive(Debug, Clone)]
pub(crate) struct Segment(Kind);

#[derive(Debug, Clone)]
enum Kind {
    Literal(Vec<u16>),
    Never,
    Wild(Vec<Item>),
}

fn is_line_terminator(unit: u16) -> bool {
    matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029)
}

fn unescape(pattern: &[u16]) -> Vec<u16> {
    let mut literal = Vec::with_capacity(pattern.len());
    let mut at = 0;
    while let Some(&unit) = pattern.get(at) {
        at += 1;
        match pattern.get(at) {
            Some(&next) if unit == BACKSLASH && !is_line_terminator(next) => {
                literal.push(next);
                at += 1;
            }
            _ => literal.push(unit),
        }
    }
    literal
}

fn items(re: &[u16]) -> Option<Vec<Item>> {
    let mut items = Vec::with_capacity(re.len());
    let mut at = 0;
    while let Some(&unit) = re.get(at) {
        at += 1;
        match unit {
            BACKSLASH => {
                items.push(Item::Unit(*re.get(at)?));
                at += 1;
            }
            OPEN => {
                let mut end = at;
                while *re.get(end)? != CLOSE {
                    end += if re[end] == BACKSLASH { 2 } else { 1 };
                }
                let class = parse_translated(&re[at..end])?;
                at = end + 1;
                if re.get(at..at + 2) == Some(&[STAR, QUESTION]) {
                    at += 2;
                    if items.last().is_none_or(|last| !matches!(last, Item::Star)) {
                        items.push(Item::Star);
                    }
                } else {
                    items.push(Item::Class(class));
                }
            }
            other => items.push(Item::Unit(other)),
        }
    }
    Some(items)
}

impl Segment {
    pub(crate) fn compile(part: &str) -> Result<Self, GlobError> {
        let pattern: Vec<u16> = part.encode_utf16().collect();
        if pattern.len() > MAX_PATTERN_UNITS {
            return Err(GlobError::PatternTooLong);
        }
        let translation = translate(&pattern, part)?;
        if !translation.magic {
            return Ok(Segment(Kind::Literal(unescape(&pattern))));
        }
        Ok(Segment(match items(&translation.re) {
            Some(items) => Kind::Wild(items),
            None => Kind::Never,
        }))
    }

    pub(crate) fn is_match(&self, name: &str) -> bool {
        match &self.0 {
            Kind::Literal(literal) => name.encode_utf16().eq(literal.iter().copied()),
            Kind::Never => false,
            Kind::Wild(items) => {
                let name: Vec<u16> = name.encode_utf16().collect();
                name.first().is_some_and(|&unit| !is_line_terminator(unit))
                    && wildcard(items, &name)
            }
        }
    }
}

fn accepts(item: &Item, unit: u16) -> bool {
    match item {
        Item::Star => false,
        Item::Unit(expected) => *expected == unit,
        Item::Class(class) => class.accepts(unit),
    }
}

fn wildcard(items: &[Item], name: &[u16]) -> bool {
    let (mut item, mut position) = (0, 0);
    let mut resume: Option<(usize, usize)> = None;
    while position < name.len() {
        match items.get(item) {
            Some(Item::Star) => {
                resume = Some((item + 1, position));
                item += 1;
            }
            Some(current) if accepts(current, name[position]) => {
                item += 1;
                position += 1;
            }
            _ => match resume {
                Some((after_star, skipped)) => {
                    item = after_star;
                    position = skipped + 1;
                    resume = Some((after_star, skipped + 1));
                }
                None => return false,
            },
        }
    }
    items[item.min(items.len())..]
        .iter()
        .all(|remaining| matches!(remaining, Item::Star))
}
