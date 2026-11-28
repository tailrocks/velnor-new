// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) type Range = (u16, u16);

const DASH: u16 = b'-' as u16;
const CARET: u16 = b'^' as u16;
const BACKSLASH: u16 = b'\\' as u16;

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Dash,
    Unit(u16),
    Set,
}

impl Token {
    fn unit(self) -> Option<u16> {
        match self {
            Token::Dash => Some(DASH),
            Token::Unit(unit) => Some(unit),
            Token::Set => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Class {
    negated: bool,
    ranges: Vec<Range>,
}

impl Class {
    pub(super) fn accepts(&self, unit: u16) -> bool {
        self.ranges
            .iter()
            .any(|&(low, high)| low <= unit && unit <= high)
            != self.negated
    }
}

pub(super) fn is_valid_raw(body: &[u16]) -> bool {
    parse(body, true).is_some()
}

pub(super) fn parse_translated(body: &[u16]) -> Option<Class> {
    parse(body, false)
}

fn parse(body: &[u16], raw: bool) -> Option<Class> {
    let (negated, body) = match body.split_first() {
        Some((&CARET, rest)) => (true, rest),
        _ => (false, body),
    };
    let tokens = tokens(body, raw)?;
    Some(Class {
        negated,
        ranges: ranges(&tokens)?,
    })
}

fn tokens(body: &[u16], raw: bool) -> Option<Vec<Token>> {
    let mut tokens = Vec::with_capacity(body.len());
    let mut at = 0;
    while let Some(&unit) = body.get(at) {
        at += 1;
        tokens.push(match unit {
            DASH => Token::Dash,
            BACKSLASH => escape(body, &mut at, raw)?,
            other => Token::Unit(other),
        });
    }
    Some(tokens)
}

fn ranges(tokens: &[Token]) -> Option<Vec<Range>> {
    let mut ranges = Vec::with_capacity(tokens.len());
    let mut at = 0;
    while let Some(&first) = tokens.get(at) {
        if let (Some(Token::Dash), Some(&last)) = (tokens.get(at + 1), tokens.get(at + 2)) {
            match (first.unit(), last.unit()) {
                (Some(low), Some(high)) if low > high => return None,
                (Some(low), Some(high)) => ranges.push((low, high)),
                (low, high) => ranges.extend(
                    [low, Some(DASH), high]
                        .into_iter()
                        .flatten()
                        .map(|unit| (unit, unit)),
                ),
            }
            at += 3;
        } else {
            if let Some(unit) = first.unit() {
                ranges.push((unit, unit));
            }
            at += 1;
        }
    }
    Some(ranges)
}

fn digit(unit: Option<&u16>, radix: u32) -> Option<u32> {
    char::from_u32(u32::from(*unit?))?.to_digit(radix)
}

fn hex(body: &[u16], at: usize, count: usize) -> Option<u16> {
    let digits = body.get(at..at + count)?;
    let value = digits.iter().try_fold(0_u32, |value, unit| {
        Some(value * 16 + digit(Some(unit), 16)?)
    })?;
    u16::try_from(value).ok()
}

fn escape(body: &[u16], at: &mut usize, raw: bool) -> Option<Token> {
    let unit = *body.get(*at)?;
    *at += 1;
    if !raw {
        return Some(Token::Unit(unit));
    }
    let letter = char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER);
    Some(match letter {
        'd' | 'D' | 's' | 'S' | 'w' | 'W' => Token::Set,
        'b' => Token::Unit(0x08),
        't' => Token::Unit(0x09),
        'n' => Token::Unit(0x0A),
        'v' => Token::Unit(0x0B),
        'f' => Token::Unit(0x0C),
        'r' => Token::Unit(0x0D),
        '0'..='7' => {
            let mut value = u32::from(unit) - u32::from(b'0');
            while let Some(next) = digit(body.get(*at), 8) {
                if value * 8 + next > 0xFF {
                    break;
                }
                value = value * 8 + next;
                *at += 1;
            }
            Token::Unit(u16::try_from(value).ok()?)
        }
        'x' => match hex(body, *at, 2) {
            Some(value) => {
                *at += 2;
                Token::Unit(value)
            }
            None => Token::Unit(unit),
        },
        'u' => match hex(body, *at, 4) {
            Some(value) => {
                *at += 4;
                Token::Unit(value)
            }
            None => Token::Unit(unit),
        },
        'c' => control(body, at),
        _ => Token::Unit(unit),
    })
}

fn control(body: &[u16], at: &mut usize) -> Token {
    let letter = body
        .get(*at)
        .and_then(|&next| u8::try_from(next).ok())
        .filter(|next| next.is_ascii_alphanumeric() || *next == b'_');
    if let Some(next) = letter {
        *at += 1;
        return Token::Unit(u16::from(next % 32));
    }
    *at -= 1;
    Token::Unit(BACKSLASH)
}
