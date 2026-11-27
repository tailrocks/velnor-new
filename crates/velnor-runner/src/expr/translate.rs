// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-FileCopyrightText: Isaac Z. Schlueter and Contributors
// SPDX-License-Identifier: Apache-2.0 AND ISC
// SPDX-FileContributor: isaacs/minimatch@3.1.2 minimatch.js

use super::js_regex::{Raw, check_raw};
use super::pattern::GlobError;

const RE_SPECIALS: &str = "().*{}+?[]^$\\!";
const STAR: &str = "[^/]*?";
const ANY: &str = "[^/]";
const UNDEFINED: &str = "undefined";
pub(super) const AHEAD: &str = "(?=.)";
const MAX_CLASS_NESTING: usize = 4_096;
const CLOSE: u16 = b']' as u16;
const AT: u16 = b'@' as u16;
const BACKSLASH: u16 = b'\\' as u16;

pub(super) struct Translation {
    pub(super) re: Vec<u16>,
    pub(super) magic: bool,
}

struct OpenClass {
    start: usize,
    re_start: usize,
}

fn units(text: &str) -> impl Iterator<Item = u16> + '_ {
    text.encode_utf16()
}

fn character(unit: u16) -> char {
    char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER)
}

struct Closers {
    last: Option<usize>,
    escaping: bool,
}

impl Closers {
    fn scan(pattern: &[u16]) -> Self {
        let mut closers = Self {
            last: None,
            escaping: false,
        };
        for (index, &unit) in pattern.iter().enumerate() {
            if unit == CLOSE && !closers.escaping {
                closers.last = Some(index);
            }
            closers.escaping = if closers.escaping {
                unit == AT
            } else {
                unit == BACKSLASH
            };
        }
        closers
    }
}

struct Translator<'a> {
    pattern: &'a [u16],
    source: &'a str,
    closers: &'a Closers,
    base: usize,
    re: Vec<u16>,
    magic: bool,
    escaping: bool,
    open: Option<OpenClass>,
    settled: bool,
}

struct Level {
    re: Vec<u16>,
    magic: bool,
    escaping: bool,
}

pub(super) fn translate(pattern: &[u16], source: &str) -> Result<Translation, GlobError> {
    let closers = Closers::scan(pattern);
    let mut levels: Vec<Level> = Vec::new();
    let mut rest = pattern;
    let tail = loop {
        let mut translator = Translator::new(rest, source, &closers, pattern.len() - rest.len());
        translator.walk()?;
        let Some(class) = translator.open.take() else {
            break translator.finish();
        };
        if levels.len() >= MAX_CLASS_NESTING {
            return Err(GlobError::ClassNestingTooDeep(MAX_CLASS_NESTING));
        }
        translator.re.truncate(class.re_start);
        translator.re.shrink_to_fit();
        levels.push(Level {
            re: translator.re,
            magic: translator.magic,
            escaping: translator.escaping,
        });
        rest = &rest[class.start + 1..];
        if is_empty_or_globstar(rest) {
            break undefined();
        }
    };
    Ok(assemble(&levels, tail))
}

fn undefined() -> Translation {
    Translation {
        re: units(UNDEFINED).collect(),
        magic: false,
    }
}

fn is_empty_or_globstar(pattern: &[u16]) -> bool {
    pattern.is_empty() || pattern.iter().copied().eq(units("**"))
}

fn assemble(levels: &[Level], tail: Translation) -> Translation {
    if levels.is_empty() {
        return tail;
    }
    let last_magic = levels.iter().rposition(|level| level.magic);
    let mut re = Vec::new();
    for (index, level) in levels.iter().enumerate() {
        if index > 0 && (tail.magic || last_magic.is_some_and(|last| last >= index)) {
            re.extend(units(AHEAD));
        }
        re.extend_from_slice(&level.re);
        re.extend(units("\\["));
    }
    if tail.magic && !tail.re.is_empty() {
        re.extend(units(AHEAD));
    }
    re.extend_from_slice(&tail.re);
    for _ in levels.iter().filter(|level| level.escaping) {
        re.extend(units("\\\\"));
    }
    Translation {
        re,
        magic: tail.magic || last_magic.is_some(),
    }
}

fn translate_inner(pattern: &[u16], source: &str) -> Result<Translation, GlobError> {
    if is_empty_or_globstar(pattern) {
        return Ok(undefined());
    }
    let inner = translate(pattern, source)?;
    if !inner.magic || inner.re.is_empty() {
        return Ok(inner);
    }
    Ok(Translation {
        re: units(AHEAD).chain(inner.re).collect(),
        magic: true,
    })
}

impl<'a> Translator<'a> {
    fn new(pattern: &'a [u16], source: &'a str, closers: &'a Closers, base: usize) -> Self {
        Self {
            pattern,
            source,
            closers,
            base,
            re: Vec::new(),
            magic: false,
            escaping: false,
            open: None,
            settled: false,
        }
    }

    fn walk(&mut self) -> Result<(), GlobError> {
        for (index, &unit) in self.pattern.iter().enumerate() {
            self.step(index, unit)?;
            if self.settled {
                break;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Translation {
        if self.escaping {
            self.re.extend(units("\\\\"));
        }
        Translation {
            re: self.re,
            magic: self.magic,
        }
    }

    fn escaped(&mut self, unit: u16) {
        self.re.extend(units("\\"));
        self.re.push(unit);
    }

    fn step(&mut self, index: usize, unit: u16) -> Result<(), GlobError> {
        let c = character(unit);
        if self.escaping && RE_SPECIALS.contains(c) {
            self.escaped(unit);
            self.escaping = false;
            return Ok(());
        }
        match c {
            '\\' => self.escaping = true,
            '?' | '*' | '+' | '@' | '!' => self.operator(index, c, unit),
            '(' if self.open.is_some() => self.re.push(unit),
            '(' | ')' => self.escaped(unit),
            '|' => {
                self.escaped(unit);
                self.escaping = false;
            }
            '[' => self.open_bracket(index, unit),
            ']' => return self.close_bracket(index, unit),
            _ => {
                if self.escaping {
                    self.escaping = false;
                } else if RE_SPECIALS.contains(c) && !(c == '^' && self.open.is_some()) {
                    self.re.extend(units("\\"));
                }
                self.re.push(unit);
            }
        }
        Ok(())
    }

    fn operator(&mut self, index: usize, c: char, unit: u16) {
        match (&self.open, c) {
            (Some(class), '!') if index == class.start + 1 => self.re.push(u16::from(b'^')),
            (Some(_), _) => self.re.push(unit),
            (None, '*') => {
                self.re.extend(units(STAR));
                self.magic = true;
            }
            (None, '?') => {
                self.re.extend(units(ANY));
                self.magic = true;
            }
            (None, _) => self.escaped(unit),
        }
    }

    fn open_bracket(&mut self, index: usize, unit: u16) {
        if self.open.is_some() {
            self.escaped(unit);
        } else {
            self.open = Some(OpenClass {
                start: index,
                re_start: self.re.len(),
            });
            self.re.push(unit);
            let first_closer = self.base + index + 2;
            if self.closers.last.is_none_or(|last| last < first_closer) {
                self.escaping = self.closers.escaping;
                self.settled = true;
            }
        }
    }

    fn close_bracket(&mut self, index: usize, unit: u16) -> Result<(), GlobError> {
        let Some(class) = self.open.take_if(|class| index != class.start + 1) else {
            self.escaped(unit);
            self.escaping = false;
            return Ok(());
        };
        let body = &self.pattern[class.start + 1..index];
        match check_raw(body) {
            Raw::Valid => {
                self.magic = true;
                self.re.push(unit);
            }
            Raw::Invalid => {
                let inner = translate_inner(body, self.source)?;
                self.re.truncate(class.re_start);
                self.re.extend(units("\\["));
                self.re.extend(inner.re);
                self.re.extend(units("\\]"));
                self.magic |= inner.magic;
            }
            Raw::Unsupported => return Err(GlobError::UnsupportedClass(self.source.to_owned())),
        }
        Ok(())
    }
}
