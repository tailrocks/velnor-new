// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::js_class::{Raw, check_raw};
use super::pattern::GlobError;

const RE_SPECIALS: &str = "().*{}+?[]^$\\!";
const STAR: &str = "[^/]*?";
const ANY: &str = "[^/]";
const UNDEFINED: &str = "undefined";

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

struct Translator<'a> {
    pattern: &'a [u16],
    source: &'a str,
    re: Vec<u16>,
    magic: bool,
    escaping: bool,
    open: Option<OpenClass>,
}

pub(super) fn translate(pattern: &[u16], source: &str) -> Result<Translation, GlobError> {
    Translator::new(pattern, source).run()
}

fn translate_inner(pattern: &[u16], source: &str) -> Result<Translation, GlobError> {
    if pattern.is_empty() || pattern.iter().copied().eq(units("**")) {
        return Ok(Translation {
            re: units(UNDEFINED).collect(),
            magic: false,
        });
    }
    Translator::new(pattern, source).run()
}

impl<'a> Translator<'a> {
    fn new(pattern: &'a [u16], source: &'a str) -> Self {
        Self {
            pattern,
            source,
            re: Vec::with_capacity(pattern.len() * 2),
            magic: false,
            escaping: false,
            open: None,
        }
    }

    fn run(mut self) -> Result<Translation, GlobError> {
        for (index, &unit) in self.pattern.iter().enumerate() {
            self.step(index, unit)?;
        }
        if let Some(class) = self.open.take() {
            let inner = translate_inner(&self.pattern[class.start + 1..], self.source)?;
            self.re.truncate(class.re_start);
            self.re.extend(units("\\["));
            self.re.extend(inner.re);
            self.magic |= inner.magic;
        }
        if self.escaping {
            self.re.extend(units("\\\\"));
        }
        Ok(Translation {
            re: self.re,
            magic: self.magic,
        })
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
