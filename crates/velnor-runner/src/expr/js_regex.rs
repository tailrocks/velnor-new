// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::js_class::is_valid_raw;

const OPEN_CLASS: u16 = b'[' as u16;
const CLOSE_CLASS: u16 = b']' as u16;
const OPEN_GROUP: u16 = b'(' as u16;
const CLOSE_GROUP: u16 = b')' as u16;
const OPEN_BRACE: u16 = b'{' as u16;
const CLOSE_BRACE: u16 = b'}' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const PIPE: u16 = b'|' as u16;
const CARET: u16 = b'^' as u16;
const DOLLAR: u16 = b'$' as u16;
const STAR: u16 = b'*' as u16;
const PLUS: u16 = b'+' as u16;
const QUESTION: u16 = b'?' as u16;
const COLON: u16 = b':' as u16;
const EQUALS: u16 = b'=' as u16;
const BANG: u16 = b'!' as u16;
const LESS: u16 = b'<' as u16;
const GREATER: u16 = b'>' as u16;
const COMMA: u16 = b',' as u16;
const ZERO: u16 = b'0' as u16;
const U: u16 = b'u' as u16;
const WORD_BOUNDARIES: [u16; 2] = [b'b' as u16, b'B' as u16];
const MODIFIERS: [u16; 4] = [b'i' as u16, b'm' as u16, b's' as u16, b'-' as u16];
const FLAG_LETTERS: &[u8] = b"ims";
const MAX_QUANTIFIER: u64 = i32::MAX as u64;

pub(super) enum Raw {
    Valid,
    Invalid,
    Unsupported,
}

#[derive(Clone, Copy, PartialEq)]
enum Previous {
    Open,
    Atom,
    Assertion,
    Quantified,
    Lazy,
}

#[derive(Clone, Copy)]
enum Group {
    Quantifiable,
    Lookbehind,
}

struct Brace {
    end: usize,
    ordered: bool,
}

struct Scanner<'a> {
    source: &'a [u16],
    at: usize,
    previous: Previous,
    groups: Vec<Group>,
    unsupported: bool,
}

pub(super) fn check_raw(body: &[u16]) -> Raw {
    let mut source = Vec::with_capacity(body.len() + 2);
    source.push(OPEN_CLASS);
    source.extend_from_slice(body);
    source.push(CLOSE_CLASS);
    Scanner {
        source: &source,
        at: 0,
        previous: Previous::Open,
        groups: Vec::new(),
        unsupported: false,
    }
    .run()
}

fn digits(source: &[u16], at: usize) -> (&[u16], usize) {
    let length = source[at..]
        .iter()
        .take_while(|&&unit| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_digit()))
        .count();
    (&source[at..at + length], at + length)
}

fn bound(digits: &[u16]) -> u64 {
    digits.iter().fold(0, |value, &unit| {
        (value * 10 + u64::from(unit - ZERO)).min(MAX_QUANTIFIER)
    })
}

fn braced(source: &[u16], at: usize) -> Option<Brace> {
    let (minimum, after_minimum) = digits(source, at);
    if minimum.is_empty() {
        return None;
    }
    match *source.get(after_minimum)? {
        CLOSE_BRACE => Some(Brace {
            end: after_minimum + 1,
            ordered: true,
        }),
        COMMA => {
            let (maximum, after_maximum) = digits(source, after_minimum + 1);
            (source.get(after_maximum) == Some(&CLOSE_BRACE)).then(|| Brace {
                end: after_maximum + 1,
                ordered: maximum.is_empty() || bound(minimum) <= bound(maximum),
            })
        }
        _ => None,
    }
}

fn identifier(name: &[u16]) -> bool {
    let mut at = 0;
    while let Some(&unit) = name.get(at) {
        let Some(byte) = u8::try_from(unit).ok().filter(u8::is_ascii) else {
            at += 1;
            continue;
        };
        if byte == b'\\' {
            let Some(length) = unicode_escape(&name[at + 1..]) else {
                return false;
            };
            at += 1 + length;
            continue;
        }
        let allowed = matches!(byte, b'$' | b'_')
            || byte.is_ascii_alphabetic()
            || (at > 0 && byte.is_ascii_digit());
        if !allowed {
            return false;
        }
        at += 1;
    }
    true
}

fn unicode_escape(rest: &[u16]) -> Option<usize> {
    let hex = |unit: &u16| char::from_u32(u32::from(*unit)).is_some_and(|c| c.is_ascii_hexdigit());
    match rest {
        [u, digits @ ..] if *u == U && digits.len() >= 4 && digits[..4].iter().all(hex) => Some(5),
        [u, open, digits @ ..] if *u == U && *open == OPEN_BRACE => {
            let length = digits.iter().take_while(|unit| hex(unit)).count();
            (length > 0 && digits.get(length) == Some(&CLOSE_BRACE)).then_some(length + 3)
        }
        _ => None,
    }
}

impl Scanner<'_> {
    fn run(mut self) -> Raw {
        while let Some(&unit) = self.source.get(self.at) {
            self.at += 1;
            if self.step(unit).is_none() {
                return Raw::Invalid;
            }
        }
        match (self.groups.is_empty(), self.unsupported) {
            (false, _) => Raw::Invalid,
            (true, true) => Raw::Unsupported,
            (true, false) => Raw::Valid,
        }
    }

    fn step(&mut self, unit: u16) -> Option<()> {
        match unit {
            PIPE => self.previous = Previous::Open,
            CARET | DOLLAR => self.previous = Previous::Assertion,
            OPEN_GROUP => self.open_group()?,
            CLOSE_GROUP => self.close_group()?,
            BACKSLASH => self.escape()?,
            OPEN_CLASS => self.class()?,
            STAR | PLUS => self.quantify()?,
            QUESTION => self.question()?,
            OPEN_BRACE => self.brace()?,
            _ => self.previous = Previous::Atom,
        }
        Some(())
    }

    fn quantify(&mut self) -> Option<()> {
        (self.previous == Previous::Atom).then(|| self.previous = Previous::Quantified)
    }

    fn question(&mut self) -> Option<()> {
        self.previous = match self.previous {
            Previous::Atom => Previous::Quantified,
            Previous::Quantified => Previous::Lazy,
            _ => return None,
        };
        Some(())
    }

    fn brace(&mut self) -> Option<()> {
        match braced(self.source, self.at) {
            Some(brace) => {
                if self.previous != Previous::Atom || !brace.ordered {
                    return None;
                }
                self.at = brace.end;
                self.previous = Previous::Quantified;
            }
            None => self.previous = Previous::Atom,
        }
        Some(())
    }

    fn escape(&mut self) -> Option<()> {
        let next = *self.source.get(self.at)?;
        self.at += 1;
        self.previous = if WORD_BOUNDARIES.contains(&next) {
            Previous::Assertion
        } else {
            Previous::Atom
        };
        Some(())
    }

    fn class(&mut self) -> Option<()> {
        let start = self.at;
        let mut end = start;
        loop {
            match *self.source.get(end)? {
                BACKSLASH => end += 2,
                CLOSE_CLASS => break,
                _ => end += 1,
            }
        }
        if !is_valid_raw(&self.source[start..end]) {
            return None;
        }
        self.at = end + 1;
        self.previous = Previous::Atom;
        Some(())
    }

    fn open_group(&mut self) -> Option<()> {
        self.previous = Previous::Open;
        if self.source.get(self.at) != Some(&QUESTION) {
            self.groups.push(Group::Quantifiable);
            return Some(());
        }
        self.at += 1;
        let kind = match *self.source.get(self.at)? {
            COLON | EQUALS | BANG => {
                self.at += 1;
                Group::Quantifiable
            }
            LESS => {
                self.at += 1;
                self.angle()?
            }
            unit if MODIFIERS.contains(&unit) => {
                self.modifiers()?;
                Group::Quantifiable
            }
            _ => return None,
        };
        self.groups.push(kind);
        Some(())
    }

    fn angle(&mut self) -> Option<Group> {
        if matches!(self.source.get(self.at), Some(&(EQUALS | BANG))) {
            self.at += 1;
            return Some(Group::Lookbehind);
        }
        self.unsupported = true;
        let rest = &self.source[self.at..];
        let length = rest.iter().position(|&unit| unit == GREATER)?;
        let name = &rest[..length];
        self.at += length + 1;
        (!name.is_empty() && identifier(name)).then_some(Group::Quantifiable)
    }

    fn modifiers(&mut self) -> Option<()> {
        self.unsupported = true;
        let mut flags = [0_u8; 2];
        let mut side = 0;
        loop {
            let unit = *self.source.get(self.at)?;
            self.at += 1;
            match u8::try_from(unit).ok()? {
                b'-' if side == 0 => side = 1,
                letter if FLAG_LETTERS.contains(&letter) => {
                    let bit = 1 << FLAG_LETTERS.iter().position(|&flag| flag == letter)?;
                    if flags[side] & bit != 0 {
                        return None;
                    }
                    flags[side] |= bit;
                }
                b':' => break,
                _ => return None,
            }
        }
        (flags[0] & flags[1] == 0 && flags[0] | flags[1] != 0).then_some(())
    }

    fn close_group(&mut self) -> Option<()> {
        self.previous = match self.groups.pop()? {
            Group::Quantifiable => Previous::Atom,
            Group::Lookbehind => Previous::Assertion,
        };
        Some(())
    }
}
